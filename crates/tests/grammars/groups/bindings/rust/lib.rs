use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_groups() -> *const ();
}

pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_groups) };
