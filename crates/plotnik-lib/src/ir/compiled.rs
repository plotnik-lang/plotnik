//! Compiled query container and buffer.
//!
//! The compiled query lives in a single contiguous allocation—cache-friendly,
//! zero fragmentation, portable to WASM. See ADR-0004 for format details.

use std::alloc::{Layout, alloc, dealloc};
use std::fmt::Write;
use std::ptr;

use super::{
    EffectOp, Entrypoint, NodeFieldId, NodeTypeId, Slice, StringId, StringRef, Transition,
    TransitionId, TypeDef, TypeMember,
};

/// Buffer alignment for cache-line efficiency.
pub const BUFFER_ALIGN: usize = 64;

/// Magic bytes identifying a compiled query file.
pub const MAGIC: [u8; 4] = *b"PLNK";

/// Current format version.
pub const FORMAT_VERSION: u32 = 1;

/// Aligned buffer for compiled query data.
///
/// Allocated via `Layout::from_size_align(len, BUFFER_ALIGN)`. Standard `Box<[u8]>`
/// won't work—it assumes 1-byte alignment and corrupts `dealloc`.
pub struct CompiledQueryBuffer {
    ptr: *mut u8,
    len: usize,
    /// `true` if allocated, `false` if mmap'd or external.
    owned: bool,
}

impl CompiledQueryBuffer {
    /// Allocate a new buffer with 64-byte alignment.
    pub fn allocate(len: usize) -> Self {
        if len == 0 {
            return Self {
                ptr: ptr::null_mut(),
                len: 0,
                owned: true,
            };
        }

        let layout = Layout::from_size_align(len, BUFFER_ALIGN).expect("invalid layout");

        // SAFETY: layout is non-zero size, properly aligned
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        Self {
            ptr,
            len,
            owned: true,
        }
    }

    /// Create a view into external memory (mmap'd or borrowed).
    ///
    /// # Safety
    /// - `ptr` must be valid for reads of `len` bytes
    /// - `ptr` must be aligned to `BUFFER_ALIGN`
    /// - The backing memory must outlive the returned buffer
    pub unsafe fn from_external(ptr: *mut u8, len: usize) -> Self {
        debug_assert!(
            (ptr as usize).is_multiple_of(BUFFER_ALIGN),
            "buffer must be 64-byte aligned"
        );
        Self {
            ptr,
            len,
            owned: false,
        }
    }

    /// Returns a pointer to the buffer start.
    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    /// Returns a mutable pointer to the buffer start.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }

    /// Returns the buffer length in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the buffer as a byte slice.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        if self.ptr.is_null() {
            &[]
        } else {
            // SAFETY: ptr is valid for len bytes if non-null
            unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
        }
    }

    /// Returns the buffer as a mutable byte slice.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        if self.ptr.is_null() {
            &mut []
        } else {
            // SAFETY: ptr is valid for len bytes if non-null, and we have &mut self
            unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
        }
    }
}

impl Drop for CompiledQueryBuffer {
    fn drop(&mut self) {
        if self.owned && !self.ptr.is_null() {
            let layout = Layout::from_size_align(self.len, BUFFER_ALIGN)
                .expect("layout was valid at allocation");
            // SAFETY: ptr was allocated with this exact layout
            unsafe { dealloc(self.ptr, layout) };
        }
    }
}

// SAFETY: The buffer is just raw bytes, safe to send across threads
unsafe impl Send for CompiledQueryBuffer {}
unsafe impl Sync for CompiledQueryBuffer {}

/// A compiled query ready for execution.
///
/// Contains a single contiguous buffer with all segments, plus offset indices
/// for O(1) access to each segment.
pub struct CompiledQuery {
    buffer: CompiledQueryBuffer,
    // Segment offsets (byte offsets into buffer)
    successors_offset: u32,
    effects_offset: u32,
    negated_fields_offset: u32,
    string_refs_offset: u32,
    string_bytes_offset: u32,
    type_defs_offset: u32,
    type_members_offset: u32,
    entrypoints_offset: u32,
    trivia_kinds_offset: u32, // 0 = no trivia kinds
    // Segment counts (number of elements)
    transition_count: u32,
    successor_count: u32,
    effect_count: u32,
    negated_field_count: u16,
    string_ref_count: u16,
    type_def_count: u16,
    type_member_count: u16,
    entrypoint_count: u16,
    trivia_kind_count: u16,
}

impl CompiledQuery {
    /// Creates a new compiled query from pre-built components.
    ///
    /// This is typically called by the emitter after layout computation.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        buffer: CompiledQueryBuffer,
        successors_offset: u32,
        effects_offset: u32,
        negated_fields_offset: u32,
        string_refs_offset: u32,
        string_bytes_offset: u32,
        type_defs_offset: u32,
        type_members_offset: u32,
        entrypoints_offset: u32,
        trivia_kinds_offset: u32,
        transition_count: u32,
        successor_count: u32,
        effect_count: u32,
        negated_field_count: u16,
        string_ref_count: u16,
        type_def_count: u16,
        type_member_count: u16,
        entrypoint_count: u16,
        trivia_kind_count: u16,
    ) -> Self {
        Self {
            buffer,
            successors_offset,
            effects_offset,
            negated_fields_offset,
            string_refs_offset,
            string_bytes_offset,
            type_defs_offset,
            type_members_offset,
            entrypoints_offset,
            trivia_kinds_offset,
            transition_count,
            successor_count,
            effect_count,
            negated_field_count,
            string_ref_count,
            type_def_count,
            type_member_count,
            entrypoint_count,
            trivia_kind_count,
        }
    }

    /// Returns the transitions segment.
    #[inline]
    pub fn transitions(&self) -> &[Transition] {
        // Transitions start at offset 0
        // SAFETY: buffer is properly aligned, transitions are at offset 0
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr() as *const Transition,
                self.transition_count as usize,
            )
        }
    }

    /// Returns the successors segment.
    #[inline]
    pub fn successors(&self) -> &[TransitionId] {
        // SAFETY: offset is aligned to 4
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr().add(self.successors_offset as usize) as *const TransitionId,
                self.successor_count as usize,
            )
        }
    }

    /// Returns the effects segment.
    #[inline]
    pub fn effects(&self) -> &[EffectOp] {
        // SAFETY: offset is aligned to 2
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr().add(self.effects_offset as usize) as *const EffectOp,
                self.effect_count as usize,
            )
        }
    }

    /// Returns the negated fields segment.
    #[inline]
    pub fn negated_fields(&self) -> &[NodeFieldId] {
        // SAFETY: offset is aligned to 2
        unsafe {
            std::slice::from_raw_parts(
                self.buffer
                    .as_ptr()
                    .add(self.negated_fields_offset as usize) as *const NodeFieldId,
                self.negated_field_count as usize,
            )
        }
    }

    /// Returns the string refs segment.
    #[inline]
    pub fn string_refs(&self) -> &[StringRef] {
        // SAFETY: offset is aligned to 4
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr().add(self.string_refs_offset as usize) as *const StringRef,
                self.string_ref_count as usize,
            )
        }
    }

    /// Returns the raw string bytes.
    #[inline]
    pub fn string_bytes(&self) -> &[u8] {
        let end = if self.type_defs_offset > 0 {
            self.type_defs_offset as usize
        } else {
            self.buffer.len()
        };
        let start = self.string_bytes_offset as usize;
        &self.buffer.as_slice()[start..end]
    }

    /// Returns the type definitions segment.
    #[inline]
    pub fn type_defs(&self) -> &[TypeDef] {
        // SAFETY: offset is aligned to 4
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr().add(self.type_defs_offset as usize) as *const TypeDef,
                self.type_def_count as usize,
            )
        }
    }

    /// Returns the type members segment.
    #[inline]
    pub fn type_members(&self) -> &[TypeMember] {
        // SAFETY: offset is aligned to 2
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr().add(self.type_members_offset as usize) as *const TypeMember,
                self.type_member_count as usize,
            )
        }
    }

    /// Returns the entrypoints segment.
    #[inline]
    pub fn entrypoints(&self) -> &[Entrypoint] {
        // SAFETY: offset is aligned to 4
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr().add(self.entrypoints_offset as usize) as *const Entrypoint,
                self.entrypoint_count as usize,
            )
        }
    }

    /// Returns the trivia kinds segment (node types to skip).
    #[inline]
    pub fn trivia_kinds(&self) -> &[NodeTypeId] {
        if self.trivia_kinds_offset == 0 {
            return &[];
        }
        // SAFETY: offset is aligned to 2
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr().add(self.trivia_kinds_offset as usize) as *const NodeTypeId,
                self.trivia_kind_count as usize,
            )
        }
    }

    /// Returns a transition by ID.
    #[inline]
    pub fn transition(&self, id: TransitionId) -> &Transition {
        &self.transitions()[id as usize]
    }

    /// Returns a view of a transition with resolved slices.
    #[inline]
    pub fn transition_view(&self, id: TransitionId) -> TransitionView<'_> {
        TransitionView {
            query: self,
            raw: self.transition(id),
        }
    }

    /// Resolves a string ID to its UTF-8 content.
    #[inline]
    pub fn string(&self, id: StringId) -> &str {
        let refs = self.string_refs();
        let string_ref = &refs[id as usize];
        let bytes = self.string_bytes();
        let start = string_ref.offset as usize;
        let end = start + string_ref.len as usize;
        // SAFETY: emitter ensures valid UTF-8
        unsafe { std::str::from_utf8_unchecked(&bytes[start..end]) }
    }

    /// Resolves a slice of effects.
    #[inline]
    pub fn resolve_effects(&self, slice: Slice<EffectOp>) -> &[EffectOp] {
        let effects = self.effects();
        let start = slice.start_index() as usize;
        let end = start + slice.len() as usize;
        &effects[start..end]
    }

    /// Resolves a slice of negated fields.
    #[inline]
    pub fn resolve_negated_fields(&self, slice: Slice<NodeFieldId>) -> &[NodeFieldId] {
        let fields = self.negated_fields();
        let start = slice.start_index() as usize;
        let end = start + slice.len() as usize;
        &fields[start..end]
    }

    /// Resolves a slice of type members.
    #[inline]
    pub fn resolve_type_members(&self, slice: Slice<TypeMember>) -> &[TypeMember] {
        let members = self.type_members();
        let start = slice.start_index() as usize;
        let end = start + slice.len() as usize;
        &members[start..end]
    }

    /// Resolves successors for a transition by ID, handling both inline and spilled cases.
    #[inline]
    pub fn resolve_successors_by_id(&self, id: TransitionId) -> &[TransitionId] {
        let transition = self.transition(id);
        if transition.has_inline_successors() {
            // Return from transitions segment - inline data is part of the transition
            let count = transition.successor_count as usize;
            &self.transitions()[id as usize].successor_data[..count]
        } else {
            let start = transition.spilled_successors_index() as usize;
            let count = transition.successor_count as usize;
            &self.successors()[start..start + count]
        }
    }

    /// Returns the number of transitions.
    #[inline]
    pub fn transition_count(&self) -> u32 {
        self.transition_count
    }

    /// Returns the number of entrypoints.
    #[inline]
    pub fn entrypoint_count(&self) -> u16 {
        self.entrypoint_count
    }

    /// Returns the raw buffer for serialization.
    #[inline]
    pub fn buffer(&self) -> &CompiledQueryBuffer {
        &self.buffer
    }

    /// Returns offset metadata for serialization.
    pub fn offsets(&self) -> CompiledQueryOffsets {
        CompiledQueryOffsets {
            successors_offset: self.successors_offset,
            effects_offset: self.effects_offset,
            negated_fields_offset: self.negated_fields_offset,
            string_refs_offset: self.string_refs_offset,
            string_bytes_offset: self.string_bytes_offset,
            type_defs_offset: self.type_defs_offset,
            type_members_offset: self.type_members_offset,
            entrypoints_offset: self.entrypoints_offset,
            trivia_kinds_offset: self.trivia_kinds_offset,
        }
    }

    /// Dumps the compiled query in human-readable format for debugging.
    pub fn dump(&self) -> String {
        let mut out = String::new();

        // Header
        writeln!(out, "CompiledQuery {{").unwrap();
        writeln!(out, "  buffer_len: {}", self.buffer.len()).unwrap();
        writeln!(out, "  transitions: {}", self.transition_count).unwrap();
        writeln!(out, "  successors: {} (spilled)", self.successor_count).unwrap();
        writeln!(out, "  effects: {}", self.effect_count).unwrap();
        writeln!(out, "  strings: {}", self.string_ref_count).unwrap();
        writeln!(out, "  type_defs: {}", self.type_def_count).unwrap();
        writeln!(out, "  entrypoints: {}", self.entrypoint_count).unwrap();
        writeln!(out).unwrap();

        // Entrypoints
        writeln!(out, "  Entrypoints:").unwrap();
        for ep in self.entrypoints() {
            let name = self.string(ep.name_id());
            writeln!(
                out,
                "    {} -> T{} (type {})",
                name,
                ep.target(),
                ep.result_type()
            )
            .unwrap();
        }
        writeln!(out).unwrap();

        // Transitions
        writeln!(out, "  Transitions:").unwrap();
        for i in 0..self.transition_count {
            let view = self.transition_view(i);
            write!(out, "    T{}: ", i).unwrap();

            // Matcher
            match view.matcher() {
                super::Matcher::Epsilon => write!(out, "ε").unwrap(),
                super::Matcher::Node { kind, field, .. } => {
                    write!(out, "Node({})", kind).unwrap();
                    if let Some(f) = field {
                        write!(out, " field={}", f).unwrap();
                    }
                }
                super::Matcher::Anonymous { kind, field, .. } => {
                    write!(out, "Anon({})", kind).unwrap();
                    if let Some(f) = field {
                        write!(out, " field={}", f).unwrap();
                    }
                }
                super::Matcher::Wildcard => write!(out, "_").unwrap(),
            }

            // Nav
            let nav = view.nav();
            if !nav.is_stay() {
                write!(out, " nav={:?}", nav.kind).unwrap();
                if nav.level > 0 {
                    write!(out, "({})", nav.level).unwrap();
                }
            }

            // Ref marker
            match view.ref_marker() {
                super::RefTransition::None => {}
                super::RefTransition::Enter(id) => write!(out, " Enter({})", id).unwrap(),
                super::RefTransition::Exit(id) => write!(out, " Exit({})", id).unwrap(),
            }

            // Effects
            let effects = view.effects();
            if !effects.is_empty() {
                write!(out, " [").unwrap();
                for (j, eff) in effects.iter().enumerate() {
                    if j > 0 {
                        write!(out, ", ").unwrap();
                    }
                    match eff {
                        EffectOp::CaptureNode => write!(out, "Capture").unwrap(),
                        EffectOp::ClearCurrent => write!(out, "Clear").unwrap(),
                        EffectOp::StartArray => write!(out, "StartArr").unwrap(),
                        EffectOp::PushElement => write!(out, "Push").unwrap(),
                        EffectOp::EndArray => write!(out, "EndArr").unwrap(),
                        EffectOp::StartObject => write!(out, "StartObj").unwrap(),
                        EffectOp::EndObject => write!(out, "EndObj").unwrap(),
                        EffectOp::Field(id) => write!(out, "Field({})", self.string(*id)).unwrap(),
                        EffectOp::StartVariant(id) => {
                            write!(out, "Var({})", self.string(*id)).unwrap()
                        }
                        EffectOp::EndVariant => write!(out, "EndVar").unwrap(),
                        EffectOp::ToString => write!(out, "ToStr").unwrap(),
                    }
                }
                write!(out, "]").unwrap();
            }

            // Successors
            let succs = view.successors();
            if !succs.is_empty() {
                write!(out, " -> [").unwrap();
                for (j, s) in succs.iter().enumerate() {
                    if j > 0 {
                        write!(out, ", ").unwrap();
                    }
                    write!(out, "T{}", s).unwrap();
                }
                write!(out, "]").unwrap();
            }

            writeln!(out).unwrap();
        }

        // Strings
        if self.string_ref_count > 0 {
            writeln!(out).unwrap();
            writeln!(out, "  Strings:").unwrap();
            for i in 0..self.string_ref_count {
                let s = self.string(i);
                writeln!(out, "    S{}: {:?}", i, s).unwrap();
            }
        }

        // Types
        if self.type_def_count > 0 {
            writeln!(out).unwrap();
            writeln!(out, "  Types:").unwrap();
            for (i, td) in self.type_defs().iter().enumerate() {
                let type_id = i as u16 + super::TYPE_COMPOSITE_START;
                let name = if td.name != super::STRING_NONE {
                    self.string(td.name)
                } else {
                    "<anon>"
                };
                write!(out, "    Ty{}: {} {:?}", type_id, name, td.kind).unwrap();
                if td.is_wrapper() {
                    if let Some(inner) = td.inner_type() {
                        write!(out, " inner=Ty{}", inner).unwrap();
                    }
                } else if let Some(members) = td.members_slice() {
                    let resolved = self.resolve_type_members(members);
                    write!(out, " {{").unwrap();
                    for (j, m) in resolved.iter().enumerate() {
                        if j > 0 {
                            write!(out, ", ").unwrap();
                        }
                        write!(out, "{}: Ty{}", self.string(m.name), m.ty).unwrap();
                    }
                    write!(out, "}}").unwrap();
                }
                writeln!(out).unwrap();
            }
        }

        writeln!(out, "}}").unwrap();
        out
    }
}

/// Offset metadata extracted from CompiledQuery.
#[derive(Debug, Clone, Copy)]
pub struct CompiledQueryOffsets {
    pub successors_offset: u32,
    pub effects_offset: u32,
    pub negated_fields_offset: u32,
    pub string_refs_offset: u32,
    pub string_bytes_offset: u32,
    pub type_defs_offset: u32,
    pub type_members_offset: u32,
    pub entrypoints_offset: u32,
    pub trivia_kinds_offset: u32,
}

/// A view of a transition with resolved slices.
///
/// Hides offset arithmetic and inline/spilled distinction from callers.
pub struct TransitionView<'a> {
    query: &'a CompiledQuery,
    raw: &'a Transition,
}

impl<'a> TransitionView<'a> {
    /// Returns the raw transition.
    #[inline]
    pub fn raw(&self) -> &'a Transition {
        self.raw
    }

    /// Returns resolved successor IDs.
    #[inline]
    pub fn successors(&self) -> &'a [TransitionId] {
        if self.raw.has_inline_successors() {
            let count = self.raw.successor_count as usize;
            &self.raw.successor_data[..count]
        } else {
            let start = self.raw.spilled_successors_index() as usize;
            let count = self.raw.successor_count as usize;
            &self.query.successors()[start..start + count]
        }
    }

    /// Returns resolved effect operations.
    #[inline]
    pub fn effects(&self) -> &'a [EffectOp] {
        self.query.resolve_effects(self.raw.effects())
    }

    /// Returns the matcher.
    #[inline]
    pub fn matcher(&self) -> &super::Matcher {
        &self.raw.matcher
    }

    /// Returns a view of the matcher with resolved slices.
    #[inline]
    pub fn matcher_view(&self) -> MatcherView<'a> {
        MatcherView {
            query: self.query,
            raw: &self.raw.matcher,
        }
    }

    /// Returns the navigation instruction.
    #[inline]
    pub fn nav(&self) -> super::Nav {
        self.raw.nav
    }

    /// Returns the ref transition marker.
    #[inline]
    pub fn ref_marker(&self) -> super::RefTransition {
        self.raw.ref_marker
    }
}

/// A view of a matcher with resolved slices.
pub struct MatcherView<'a> {
    query: &'a CompiledQuery,
    raw: &'a super::Matcher,
}

impl<'a> MatcherView<'a> {
    /// Returns the raw matcher.
    #[inline]
    pub fn raw(&self) -> &'a super::Matcher {
        self.raw
    }

    /// Returns resolved negated fields.
    #[inline]
    pub fn negated_fields(&self) -> &'a [NodeFieldId] {
        self.query.resolve_negated_fields(self.raw.negated_fields())
    }

    /// Returns the matcher kind.
    #[inline]
    pub fn kind(&self) -> super::MatcherKind {
        self.raw.kind()
    }
}

/// Aligns an offset up to the given alignment.
#[inline]
pub const fn align_up(offset: u32, align: u32) -> u32 {
    (offset + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_alignment() {
        let buf = CompiledQueryBuffer::allocate(128);
        assert_eq!(buf.as_ptr() as usize % BUFFER_ALIGN, 0);
        assert_eq!(buf.len(), 128);
    }

    #[test]
    fn buffer_empty() {
        let buf = CompiledQueryBuffer::allocate(0);
        assert!(buf.is_empty());
        assert_eq!(buf.as_slice(), &[] as &[u8]);
    }

    #[test]
    fn align_up_values() {
        assert_eq!(align_up(0, 4), 0);
        assert_eq!(align_up(1, 4), 4);
        assert_eq!(align_up(4, 4), 4);
        assert_eq!(align_up(5, 4), 8);
        assert_eq!(align_up(63, 64), 64);
        assert_eq!(align_up(64, 64), 64);
        assert_eq!(align_up(65, 64), 128);
    }
}
