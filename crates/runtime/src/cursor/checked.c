/* Navigation adapted from tree-sitter 0.27.0 tree_cursor.c, MIT licensed.
 * Copyright (c) 2018-2025 Max Brunsfeld, Amaan Qureshi, and contributors.
 * https://github.com/tree-sitter/tree-sitter/blob/v0.27.0/LICENSE
 */
#include "tree_sitter/api.h"
#include "tree_cursor.h"
#include "language.h"
#include "tree.h"

_Static_assert(sizeof(TreeCursor) <= sizeof(TSTreeCursor), "cursor layout");

typedef bool (*Grow)(void *, void **, uint32_t *, uint32_t, uint32_t, size_t, size_t, uint64_t *);
typedef struct {
  TreeCursor *cursor;
  uint64_t *fuel;
  Grow grow;
  void *context;
  bool failed;
} Run;

static bool tick(Run *run) {
  if (!*run->fuel) { run->failed = true; return false; }
  --*run->fuel;
  return true;
}

static bool reserve(Run *run, uint32_t required) {
  TreeCursor *cursor = run->cursor;
  if (required <= cursor->stack.capacity) return true;
  if (!run->grow(run->context, (void **)&cursor->stack.contents,
      &cursor->stack.capacity, cursor->stack.size, required,
      sizeof(TreeCursorEntry), _Alignof(TreeCursorEntry), run->fuel)) {
    run->failed = true;
    return false;
  }
  return true;
}

static bool push(Run *run, TreeCursorEntry entry) {
  if (run->cursor->stack.size == UINT32_MAX || !reserve(run, run->cursor->stack.size + 1)) {
    run->failed = true;
    return false;
  }
  run->cursor->stack.contents[run->cursor->stack.size++] = entry;
  return true;
}

static bool visible(const TreeCursor *cursor, uint32_t index) {
  const TreeCursorEntry *entry = &cursor->stack.contents[index];
  if (!index || ts_subtree_visible(*entry->subtree)) return true;
  if (ts_subtree_extra(*entry->subtree)) return false;
  const TreeCursorEntry *parent = &cursor->stack.contents[index - 1];
  return ts_language_alias_at(cursor->tree->language,
    parent->subtree->ptr->production_id, entry->structural_child_index) != 0;
}

typedef struct {
  Subtree parent;
  Length position;
  uint32_t child_index, structural_child_index, descendant_index;
  const TSSymbol *aliases;
} Children;

static Children children(const TreeCursor *cursor) {
  const TreeCursorEntry *entry = &cursor->stack.contents[cursor->stack.size - 1];
  if (!ts_subtree_child_count(*entry->subtree)) return (Children){0};
  return (Children){
    .parent = *entry->subtree,
    .position = entry->position,
    .descendant_index = entry->descendant_index + visible(cursor, cursor->stack.size - 1),
    .aliases = ts_language_alias_sequence(cursor->tree->language, entry->subtree->ptr->production_id),
  };
}

static bool next(Run *run, Children *iterator, TreeCursorEntry *entry, bool *is_visible) {
  if (!iterator->parent.ptr || iterator->child_index == iterator->parent.ptr->child_count) return false;
  if (!tick(run)) return false;
  const Subtree *child = &ts_subtree_children(iterator->parent)[iterator->child_index];
  *entry = (TreeCursorEntry){child, iterator->position, iterator->child_index,
    iterator->structural_child_index, iterator->descendant_index};
  *is_visible = ts_subtree_visible(*child);
  if (!ts_subtree_extra(*child)) {
    if (iterator->aliases) *is_visible |= iterator->aliases[iterator->structural_child_index] != 0;
    ++iterator->structural_child_index;
  }
  iterator->descendant_index += ts_subtree_visible_descendant_count(*child) + *is_visible;
  iterator->position = length_add(iterator->position, ts_subtree_size(*child));
  if (++iterator->child_index < iterator->parent.ptr->child_count) {
    iterator->position = length_add(iterator->position,
      ts_subtree_padding(ts_subtree_children(iterator->parent)[iterator->child_index]));
  }
  return true;
}

static bool first_child(Run *run) {
  for (;;) {
    if (!tick(run)) return false;
    Children iterator = children(run->cursor);
    TreeCursorEntry entry;
    bool is_visible, descended = false;
    while (next(run, &iterator, &entry, &is_visible)) {
      if (is_visible || ts_subtree_visible_child_count(*entry.subtree)) {
        if (!push(run, entry)) return false;
        if (is_visible) return true;
        descended = true;
        break;
      }
    }
    if (!descended || run->failed) return false;
  }
}

static bool sibling(Run *run) {
  TreeCursor *cursor = run->cursor;
  uint32_t initial_size = cursor->stack.size;
  while (cursor->stack.size > 1) {
    if (!tick(run)) return false;
    TreeCursorEntry entry = cursor->stack.contents[--cursor->stack.size];
    Children iterator = children(cursor);
    iterator.child_index = entry.child_index;
    iterator.structural_child_index = entry.structural_child_index;
    iterator.position = entry.position;
    iterator.descendant_index = entry.descendant_index;
    bool is_visible = false;
    next(run, &iterator, &entry, &is_visible);
    if (run->failed) return false;
    if (is_visible && cursor->stack.size + 1 < initial_size) break;
    while (next(run, &iterator, &entry, &is_visible)) {
      if (is_visible || ts_subtree_visible_child_count(*entry.subtree)) {
        if (!push(run, entry)) return false;
        return is_visible || first_child(run);
      }
    }
    if (run->failed) return false;
  }
  cursor->stack.size = initial_size;
  return false;
}

static bool parent(Run *run) {
  TreeCursor *cursor = run->cursor;
  for (uint32_t i = cursor->stack.size - 1; i > 0;) {
    if (!tick(run)) return false;
    if (visible(cursor, --i)) { cursor->stack.size = i + 1; return true; }
  }
  return false;
}

static void seek(Run *run, uint32_t goal) {
  TreeCursor *cursor = run->cursor;
  for (;;) {
    if (!tick(run)) return;
    uint32_t i = cursor->stack.size - 1;
    TreeCursorEntry entry = cursor->stack.contents[i];
    uint64_t end = (uint64_t)entry.descendant_index + visible(cursor, i)
      + ts_subtree_visible_descendant_count(*entry.subtree);
    if (entry.descendant_index <= goal && end > goal) break;
    if (cursor->stack.size <= 1) return;
    --cursor->stack.size;
  }
  for (;;) {
    if (!tick(run)) return;
    Children iterator = children(cursor);
    if (iterator.descendant_index > goal) return;
    TreeCursorEntry entry;
    bool is_visible, descended = false;
    while (next(run, &iterator, &entry, &is_visible)) {
      if (iterator.descendant_index > goal) {
        if (!push(run, entry)) return;
        if (is_visible && entry.descendant_index == goal) return;
        descended = true;
        break;
      }
    }
    if (!descended || run->failed) return;
  }
}

TSNode plotnik_cursor_node(const TSTreeCursor *raw) {
  const TreeCursor *cursor = (const TreeCursor *)raw;
  const TreeCursorEntry *entry = &cursor->stack.contents[cursor->stack.size - 1];
  bool extra = ts_subtree_extra(*entry->subtree);
  TSSymbol alias = extra ? 0 : cursor->root_alias_symbol;
  if (cursor->stack.size > 1 && !extra) {
    const TreeCursorEntry *parent = &cursor->stack.contents[cursor->stack.size - 2];
    alias = ts_language_alias_at(cursor->tree->language,
      parent->subtree->ptr->production_id, entry->structural_child_index);
  }
  return ts_node_new(cursor->tree, entry->subtree, entry->position, alias);
}

uint32_t plotnik_cursor_index(const TSTreeCursor *raw) {
  const TreeCursor *cursor = (const TreeCursor *)raw;
  return cursor->stack.contents[cursor->stack.size - 1].descendant_index;
}

/* Operations: reset, first child, next sibling, parent, seek, copy,
 * requested-field membership, path-extra classification. Failure invalidates
 * the position until reset, but never the owned storage. */
int plotnik_cursor_run(TSTreeCursor *raw, unsigned operation, TSNode node,
    uint32_t operand, const TSTreeCursor *source, uint64_t *fuel, Grow grow, void *context) {
  TreeCursor *cursor = (TreeCursor *)raw;
  Run run = {cursor, fuel, grow, context, false};
  bool result = true;
  if (!tick(&run)) return -1;
  switch (operation) {
    case 0:
      if (!reserve(&run, 1)) break;
      cursor->tree = node.tree;
      cursor->root_alias_symbol = node.context[3];
      cursor->stack.size = 1;
      cursor->stack.contents[0] = (TreeCursorEntry){
        .subtree = (const Subtree *)node.id,
        .position = {ts_node_start_byte(node), ts_node_start_point(node)},
      };
      break;
    case 1: result = first_child(&run); break;
    case 2: result = sibling(&run); break;
    case 3: result = parent(&run); break;
    case 4: seek(&run, operand); break;
    case 5: {
      const TreeCursor *other = (const TreeCursor *)source;
      if (!reserve(&run, other->stack.size)) break;
      uint64_t cost = ((uint64_t)other->stack.size * sizeof(TreeCursorEntry) + 63) / 64;
      if (*fuel < cost) { run.failed = true; break; }
      *fuel -= cost;
      memcpy(cursor->stack.contents, other->stack.contents, other->stack.size * sizeof(TreeCursorEntry));
      cursor->stack.size = other->stack.size;
      cursor->tree = other->tree;
      cursor->root_alias_symbol = other->root_alias_symbol;
      break;
    }
    case 6:
    case 7: {
      bool found = false, extra = false;
      for (uint32_t i = cursor->stack.size - 1; i > 0; --i) {
        if (!tick(&run)) break;
        if (i != cursor->stack.size - 1 && visible(cursor, i)) break;
        const TreeCursorEntry *entry = &cursor->stack.contents[i];
        extra |= ts_subtree_extra(*entry->subtree);
        if (operation == 7) continue;
        const TreeCursorEntry *parent = &cursor->stack.contents[i - 1];
        const TSFieldMapEntry *map, *end;
        ts_language_field_map(cursor->tree->language, parent->subtree->ptr->production_id, &map, &end);
        for (; map < end; ++map) {
          if (!tick(&run)) break;
          found |= !map->inherited && map->child_index == entry->structural_child_index && map->field_id == operand;
        }
        if (run.failed) break;
      }
      result = operation == 7 ? extra : found && !extra;
      break;
    }
    default: abort();
  }
  return run.failed ? -1 : result;
}
