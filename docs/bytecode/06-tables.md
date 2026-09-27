# Tables

- [Design](#design)
- [Implementation](#implementation)
  - [Strings](#strings)
  - [Node kinds and fields](#node-kinds-and-fields)
  - [Regexes](#regexes)
  - [Sparse DFA](#sparse-dfa)
    - [Framing](#framing)
    - [Flags and alphabet](#flags-and-alphabet)
    - [Special-state bounds](#special-state-bounds)
    - [Sparse states](#sparse-states)
    - [Start table](#start-table)
    - [Quit-byte set](#quit-byte-set)
    - [Regex predicate meaning](#regex-predicate-meaning)

## Design

A string ID selects a row in `StringTable`. That row gives the string's starting byte offset in `StringBlob`, and the next row gives its end. The table can locate strings of any length without storing a length in every reference.

For example, two neighboring entries might contain `if` and `while`. Their boundaries meet, so the end of the first string is also the start of the second:

```text
 Start of "if"         Start of "while"          Next offset
      |                       |                       |
      v                       v                       v
      +-----------------------+-----------------------+
      | if                    | while                 |
      +-----------------------+-----------------------+
```

The last string needs an end boundary too. A final table row, the sentinel, supplies that offset without representing another string. `RegexTable` uses the same boundary scheme for compiled regular expressions in `RegexBlob`.

A regex entry pairs a compiled DFA for matching with a reference to the pattern's text. Loading uses the DFA directly. The pattern text remains descriptive metadata.

The sparse DFA groups input bytes into classes and stores ranges of classes that lead to the same destination. A transition depends on both the current state and the next byte:

```text
Input byte --> Byte class --+
                            |
                            +--> Stored ranges --> Next state
                            |
Current state --------------+
```

A state can share one destination across a range instead of storing a destination for every possible byte. A class omitted from its ranges leads to the dead state, where the search ends.

`NodeKinds` and `NodeFields` serve a different purpose. Instructions already contain IDs from the source-language grammar. These tables attach names to those IDs. Their row numbers do not replace the grammar IDs used for matching.

## Implementation

### Strings

`StringBlob` contains UTF-8 strings without terminators. `StringTable` has one little-endian `u32` offset per counted string, followed by one sentinel offset. The header's `str_table_count` excludes the sentinel.

Each string starts at its offset and ends just before the next offset. Offsets never decrease or exceed `str_blob_size`. The sentinel equals `str_blob_size`, ending the last string at the blob's end:

```text
           Last string's offset       Sentinel: str_blob_size
                     |                         |
StringBlob ... ------+-------------------------+
                     | Last string's bytes     |
```

Equal adjacent offsets represent an empty string. The sentinel is not a string. Every string is valid UTF-8, including entry 0.

A `StringId` is a nonzero `StringTable` index below `str_table_count`. Grammar names, type names, member names, entry-point names, and real regex pattern names use these IDs. [String predicates](02-instructions.md#predicates) have a separate operand range.

Writers emit 1 through 65534 counted strings and start the first at offset zero. Entry 0 is the 25-byte ASCII string `Beauty will save the world`. A modest ambition for a reserved entry. Real strings start at index 1 and share an entry when their bytes are identical. A real string equal to entry 0 still has a separate nonzero ID.

The reader accepts counts from 0 through 65535, arbitrary contents at entry 0, duplicate strings, and a nonzero first offset. Bytes before that offset are outside every string and are uninterpreted. With count zero, the table contains only the sentinel and the whole blob is uninterpreted. The largest accepted real string ID is 65534.

### Node kinds and fields

`NodeKinds` and `NodeFields` each contain four-byte records:

| Byte offset | Width | Field             |
| ----------- | ----- | ----------------- |
| 0           | `u16` | Grammar symbol ID |
| 2           | `u16` | Name `StringId`   |

`NodeKinds` accepts IDs 1 through `0xFFFD`. Zero is the grammar's end symbol, and `0xFFFE` and `0xFFFF` are built-in error symbols. They are invalid in this table.

`NodeFields` accepts IDs 1 through `0xFFFF`. Zero means no field and is invalid in this table. Both tables require nonzero name IDs.

These records attach names to grammar IDs. An instruction uses the grammar ID directly, not the metadata row's index. The reader does not require sorted or unique records, completeness, or correspondence with an external grammar. The bytecode contains no grammar identity or definition.

### Regexes

`RegexBlob` contains serialized sparse DFAs. `RegexTable` has one eight-byte record per counted regex, followed by one sentinel record. The header's `regex_table_count` excludes the sentinel.

```text
0                       2                 4                     8
+-----------------------+-----------------+---------------------+
| pattern_string: u16   | reserved: u16   | offset: u32         |
+-----------------------+-----------------+---------------------+
```

`pattern_string` references `StringTable`. `reserved` is zero in every record, including entry 0 and the sentinel. `offset` counts bytes from the beginning of `RegexBlob`. Offsets never decrease or exceed `regex_blob_size`. The sentinel's offset equals `regex_blob_size`.

Each regex occupies the bytes from its offset up to the next offset:

```text
Entry offset          End of DFA               Next offset
     |                    |                       |
     +--------------------+-----------------------+
     | Sparse DFA         | Extra bytes           |
     +--------------------+-----------------------+
```

The final regex ends at the sentinel offset. A real `RegexId` is nonzero and below `regex_table_count`. Each real entry has a nonzero `pattern_string` and a complete valid [sparse DFA](#sparse-dfa) at the start of its bytes. The DFA determines matching. The pattern string is descriptive metadata that the reader neither compiles nor compares with the DFA.

Writers emit 1 through 65535 counted records. Their entry 0 and sentinel are:

| Record   | `pattern_string` | `reserved` | `offset`          |
| -------- | ---------------- | ---------- | ----------------- |
| Entry 0  | 0                | 0          | 0                 |
| Sentinel | 0                | 0          | `regex_blob_size` |

Entry 0 has no DFA bytes. The first real DFA begins at blob offset zero. Each later DFA starts at the first multiple of 4 at or after the preceding DFA's encoded end. Inter-DFA padding is zero. The blob ends immediately after its final DFA and is empty when there are no real regexes.

Writers reuse one `RegexId` for real entries with the same pattern string ID. Their DFA integers are little-endian. Each DFA represents one pattern and supports unanchored search. The writer sets no quit bytes, so no input byte is marked as unsupported.

The reader also accepts count zero, arbitrary `pattern_string` values at entry 0 and the sentinel, and a nonzero first offset. Bytes before the first offset and bytes belonging to entry 0 are uninterpreted. The reader imposes no DFA alignment and ignores any bytes after a complete DFA, even if nonzero. Those bytes belong to `RegexBlob`, so the section-padding rule does not apply to them.

### Sparse DFA

Plotnik embeds the sparse-DFA codec from `regex-automata`. Alongside state transitions, it stores a start table for choosing the initial state and special-state bounds for recognizing states that report a match, stop a search, or allow an accelerated scan. Structures inside a DFA have no implicit alignment. Multibyte fields can begin at any byte offset.

#### Framing

The prefix identifies the codec and its byte order. Offsets in these tables are measured from the first byte of the DFA.

| Byte offset | Width    | Field              |
| ----------- | -------- | ------------------ |
| 0           | 30 bytes | Label              |
| 30          | `u8`     | Zero terminator    |
| 31          | `u8`     | Label padding      |
| 32          | `u32`    | Endianness marker  |
| 36          | `u32`    | Codec version, `2` |
| 40          | `u32`    | Unused             |
| 44          | `u32`    | Flags              |

The label is ASCII `rust-regex-automata-dfa-sparse` and the marker is `0x0000FEFF`. Writers also zero the label padding, unused word, and flag bits 3 through 31. The reader ignores those three reserved areas.

Writers encode all DFA integers little-endian, including the two `u128` quit-set values. The reader interprets integers in the host's byte order and requires the resulting marker to equal `0xFEFF`. It accepts little-endian DFAs on a little-endian host and big-endian DFAs on a big-endian host. A DFA in the other byte order is rejected. The surrounding Plotnik fields remain little-endian.

The prefix is followed by counts, the byte-class map, and the length of the sparse-state area:

| Byte offset | Width     | Field                    |
| ----------- | --------- | ------------------------ |
| 48          | `u32`     | `state_count`            |
| 52          | `u32`     | `pattern_count`          |
| 56          | 256 bytes | Byte-class map           |
| 312         | `u32`     | Sparse-state byte length |

`state_count` counts the records in that area. `pattern_count` declares how many patterns the DFA represents. A matching state names the patterns it matched by pattern ID. Plotnik writers use one pattern. The reader accepts every `u32` pattern count and does not compare it with match-state pattern IDs or the start table's pattern count.

The remaining structures are consecutive. The stored byte length ends the sparse-state area and locates the start table. That table chooses an initial state for anchored or unanchored search. Its row count locates the fixed-size blocks after it.

```text
316 +-----------------------------------------------------+
    | Sparse states                                       |
    | Byte length is stored at offset 312                 |
    +-----------------------------------------------------+
    | Start table                                         |
    |   276-byte header                                   |
    |   Two mode rows, 24 bytes each                      |
    |   Optional pattern-specific rows, 24 bytes each     |
    +-----------------------------------------------------+
    | Special-state bounds: eight u32 values, 32 bytes    |
    +-----------------------------------------------------+
    | Quit-byte set: two u128 values, 32 bytes            |
    +-----------------------------------------------------+ DFA end
```

Every structure fits inside the regex's byte range. The DFA ends immediately after the quit-byte set, with no final codec padding.

#### Flags and alphabet

The flags word at DFA byte 44 has three defined bits:

- Bit 0: the automaton can match an empty string.
- Bit 1: matches respect UTF-8 boundaries, including empty matches.
- Bit 2: the pattern is anchored at the start of the text, regardless of search mode.

The byte-class map contains one `u8` class per input byte, in byte-value order. A state's transition ranges refer to these class numbers, not directly to input bytes. For example, if `a`, `b`, and `c` share class 4, a range covering classes 3 through 5 handles all three alike:

```text
'a' --+
      |
'b' --+--> class 4 --> range 3 through 5 --> same destination
      |
'c' --+
```

The alphabet size is two more than the class stored for byte `0xFF`. One symbol is reserved for end of input. Writers assign contiguous classes in byte order starting at zero, with end of input as the final symbol. The reader only requires each map entry to be less than the alphabet size. It does not require monotone or contiguous classes.

#### Special-state bounds

A DFA state ID is the byte offset of its record within the sparse-state area, which starts at DFA offset 316. For example, a nine-byte record at state ID 0 is followed by state ID 9. The records fill the declared area exactly, with no gaps, and their count equals `state_count`. State 0 is dead.

State IDs and pattern IDs range from zero through `0x7FFFFFFE`. The sparse-state byte length ranges from 1 through `0x7FFFFFFE`, so the offset immediately after the last state also fits the ID range.

The special-state block classifies state IDs by ranges. Dead and quit states stop a search, while matching states report a result. Accelerated states allow a search to skip input that would loop back to the same state. Special start states mark places where search can look ahead for a possible match start. These classifications apply during search, even when a record has ordinary-looking transitions. The block contains eight `u32` IDs in this order:

| Index | Field         |
| ----- | ------------- |
| 0     | `max_special` |
| 1     | `quit`        |
| 2     | `min_match`   |
| 3     | `max_match`   |
| 4     | `min_accel`   |
| 5     | `max_accel`   |
| 6     | `min_start`   |
| 7     | `max_start`   |

| Class         | IDs, inclusive                  |
| ------------- | ------------------------------- |
| Special       | 0 through `max_special`         |
| Dead          | 0                               |
| Quit          | `quit`, when nonzero            |
| Match         | `min_match` through `max_match` |
| Accelerated   | `min_accel` through `max_accel` |
| Special start | `min_start` through `max_start` |

Each minimum/maximum pair is either entirely zero, meaning no range, or entirely nonzero. State 0 is excluded from the three ranges. The special start range classifies states for search and does not replace the start table. `quit` is zero when there is no quit state.

The reader checks every field against the numeric state-ID range. `max_special` is below the sparse-state byte length and at least `quit` and every range maximum. Each present range starts after `quit`, and its minimum does not exceed its maximum.

Present range minima never decrease in the order match, accelerator, start. Their maxima need not follow that order, and ranges can overlap. Along with the state-record checks, these are the complete special-bound checks. Range endpoints and `quit` need not identify state beginnings.

Writers set `max_special` to the largest of `quit` and the three maxima. Every nonzero writer endpoint and quit ID identifies a state record.

#### Sparse states

Each state starts with a `u16` header:

```text
bit 15                 bits 14..0
+----------------------+--------------------------------------+
| Match flag           | Transition count                     |
+----------------------+--------------------------------------+
```

The transition count is 1 through 257, including one end-of-input transition. Each transition has a range and a destination stored in separate arrays. Matching states also list pattern IDs. The final accelerator bytes identify input bytes whose transitions leave this state, which permits an optional search shortcut. The fields follow in this order:

```text
+--------------------------------------------------------+
| Input ranges: one pair of u8 endpoints per transition  |
+--------------------------------------------------------+
| Destinations: one u32 state ID per transition          |
+--------------------------------------------------------+
| Only when the match flag is set:                       |
|   Pattern-ID count: u32, greater than zero             |
|   Pattern IDs: one u32 per counted pattern             |
+--------------------------------------------------------+
| Accelerator-byte count: u8, from 0 through 3           |
| Accelerator bytes: one byte per counted item           |
+--------------------------------------------------------+
```

Each input range stores its lower endpoint first and upper endpoint second, both inclusive. Ranges and destinations pair by their position in the two arrays. All but the last range select transitions for byte-class values. The last destination is the end-of-input transition, and its range pair is unused. If several ranges contain a class, the first in stored order selects the destination. A class covered by no range selects state 0.

Writers emit increasing, nonoverlapping ranges, omit byte transitions to dead state, and zero both endpoints of the unused end-of-input pair. Accelerator bytes are raw input bytes, not class numbers, and list all bytes whose transition leaves the state. Search can use the transitions directly without this optimization.

The [special-state bounds](#special-state-bounds) classify IDs as matching, accelerated, or otherwise special. The reader checks the state records against those bounds and their own fields:

- Every lower endpoint is at most its upper endpoint, including in the unused pair. Range ordering, overlap, class bounds, and a zero unused pair are not checked.
- Every destination identifies the beginning of a state record, never an interior byte or the end of the area.
- The match flag is set exactly for states in the special match range.
- Every pattern ID fits the ID range. IDs need not be distinct, ordered, or below `pattern_count`.
- The accelerator count is positive exactly for states in the special accelerator range. Byte uniqueness and agreement with the transitions are not checked.
- The end-of-input destination is not the nonzero quit-state ID.
- Every state at or below `max_special` is dead, quit, matching, accelerated, or in the special start range. A state can belong to several ranges.

The reader does not require dead or quit states to have self-loops. An ordinary byte transition into dead ends the search, and one into quit reports an unsupported search. Either state can be selected as an initial state, where its first byte or end-of-input transition still applies before those stopping rules take effect.

#### Start table

The start state can depend on whether a search is anchored and on the byte immediately before it. Anchored search starts only at the requested position. Unanchored search can also find a later start, subject to the pattern's own anchors.

The start table stores a row for each search mode. Each row has six state IDs, one per context. Optional rows select anchored search for individual patterns.

The header declares the supported modes, maps preceding bytes to contexts, and gives the number of pattern-specific rows. It also has an optional universal ID for each mode. Such an ID advertises one start state that works for every context in that mode.

Offsets below are relative to the start table, immediately after the sparse states:

| Byte offset | Width     | Field                      |
| ----------- | --------- | -------------------------- |
| 0           | `u32`     | Start kind                 |
| 4           | 256 bytes | Look-behind map            |
| 260         | `u32`     | Stride                     |
| 264         | `u32`     | Pattern-specific row count |
| 268         | `u32`     | Universal unanchored ID    |
| 272         | `u32`     | Universal anchored ID      |

The start kind selects the supported search modes:

| Start kind | Modes           |
| ---------- | --------------- |
| 0          | Both            |
| 1          | Unanchored only |
| 2          | Anchored only   |

Other start kinds are invalid. Stride is exactly 6, the number of state IDs in each row. The pattern-specific count and either universal ID use `0xFFFFFFFF` to mean absent. An absent count means no pattern-specific rows. A present count ranges from zero through `0x7FFFFFFF`.

Rows begin at byte 276. Each occupies 24 bytes and contains six `u32` state IDs. Both mode rows are present even when their mode is unsupported. Pattern-specific rows follow in pattern-index order:

```text
276 +---------------------------------------+
    | Unanchored row: six state IDs         |
300 +---------------------------------------+
    | Anchored row: six state IDs           |
324 +---------------------------------------+
    | Pattern-specific rows, when present   |
    | One row per pattern, in index order   |
    +---------------------------------------+
```

The columns describe the context immediately before the search:

| Column | Context                              |
| ------ | ------------------------------------ |
| 0      | Non-word byte                        |
| 1      | ASCII word byte                      |
| 2      | Beginning of text, no preceding byte |
| 3      | Line feed, byte `0x0A`               |
| 4      | Carriage return, byte `0x0D`         |
| 5      | Custom line terminator               |

The look-behind map has one byte per input byte, in byte-value order. Its entry for the preceding byte selects the column. With no preceding byte, column 2 applies. Pattern-specific starts have no universal ID.

Writer maps assign ASCII letters, digits, and `_` to column 1, line feed to 3, carriage return to 4, and other bytes to 0. Plotnik does not select a custom line terminator. Writer universal IDs agree with their mode's row. Plotnik output has start kind 1 and no pattern-specific rows.

The reader checks map entries and row IDs, including rows for unsupported modes. Map entries are from 0 through 5. Every row ID identifies a state record outside the special match range. The reader does not check the map's semantic classification.

A present universal ID only needs to fit the numeric state-ID range. The reader does not require it to identify a state, agree with its row, or be nonmatching.

#### Quit-byte set

Quit bytes mark input the DFA does not support. The final 32 bytes store the set as two consecutive `u128` bitsets:

```text
+--------------------------------+--------------------------------+
| low                            | high                           |
| bit 0   selects byte 0         | bit 0   selects byte 128       |
| ...                            | ...                            |
| bit 127 selects byte 127       | bit 127 selects byte 255       |
+--------------------------------+--------------------------------+
```

Bits correspond to consecutive byte values, with bit 0 the least significant bit. A set bit marks that byte as a quit byte. A look-behind byte in the set makes the requested search unsupported. During search, quit transitions identify inputs for which the DFA cannot answer.

The reader accepts every bit pattern without checking its relation to state transitions. Plotnik writers zero both bitsets.

The reader decodes both logical bitsets from the first stored bitset. The second is consumed but ignored. This differs from the two-bitset wire meaning above. Empty quit sets are unaffected.

#### Regex predicate meaning

A regex predicate tests whether the DFA finds any match in the selected node's complete UTF-8 text. Search begins in the unanchored row's text-start column, without a preceding byte. The transition graph accounts for possible match starts throughout the text. No match group or matched substring becomes a predicate result.

Ordinary byte transitions use the byte-class map. One end-of-input transition follows the final text byte. The DFA reports a match one transition after the match ends:

| Matching state reached by | Reported match end           |
| ------------------------- | ---------------------------- |
| Ordinary byte             | Immediately before that byte |
| End-of-input transition   | End of the text              |

For example, a DFA matching `a` needs the end-of-input transition to report a match when the text is just `a`. An ordinary byte transition into dead ends the search. With both empty-match and UTF-8 flags set, match positions inside a multibyte character are excluded. An eligible match establishes a positive predicate result.

Plotnik's writer encodes the query language's regex meaning, with UTF-8 mode set, captures erased, ASCII word boundaries, and no quit conditions. Anchors remain properties of the transition graph.

The reader checks structural constraints, not equivalence to the pattern text or every condition required for successful unanchored execution. An anchored-only DFA, inconsistent optimization metadata, or a reachable quit condition can pass structural decoding. Structural acceptance alone does not guarantee that the DFA can execute a Plotnik predicate.
