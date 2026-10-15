use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Symbol(u32);

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Interner {
    strings: IndexSet<String, FxBuildHasher>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(idx) = self.strings.get_index_of(s) {
            return Symbol(idx as u32);
        }

        let (idx, _) = self.strings.insert_full(s.to_owned());
        Symbol(idx as u32)
    }

    pub fn get(&self, s: &str) -> Option<Symbol> {
        self.strings.get_index_of(s).map(|i| Symbol(i as u32))
    }

    #[inline]
    pub fn resolve(&self, sym: Symbol) -> &str {
        self.strings
            .get_index(sym.0 as usize)
            .expect("symbol was not created by this interner")
    }

    #[inline]
    pub fn try_resolve(&self, sym: Symbol) -> Option<&str> {
        self.strings.get_index(sym.0 as usize).map(String::as_str)
    }
}
