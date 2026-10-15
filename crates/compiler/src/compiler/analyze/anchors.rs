use crate::bytecode::Nav;
use crate::compiler::analyze::shape::PatternFacts;
use crate::compiler::parse::ast::{Pattern, SeqItem};
use crate::core::{NodeClass, SkipClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GapClass {
    Any,
    AnonymousAndExtras,
    ExtrasOnly,
    Exact,
}

impl GapClass {
    pub(crate) fn skip_class(self) -> SkipClass {
        match self {
            Self::Any => SkipClass::Any,
            Self::AnonymousAndExtras => SkipClass::Trivia,
            Self::ExtrasOnly => SkipClass::Extras,
            Self::Exact => SkipClass::Exact,
        }
    }

    pub(crate) fn admits(self, node: NodeClass) -> bool {
        self.skip_class().admits(node)
    }

    fn permissiveness(self) -> u8 {
        match self {
            Self::Exact => 0,
            Self::ExtrasOnly => 1,
            Self::AnonymousAndExtras => 2,
            Self::Any => 3,
        }
    }

    pub fn tighten(self, other: Self) -> Self {
        if self.permissiveness() <= other.permissiveness() {
            self
        } else {
            other
        }
    }

    pub fn from_nav(nav: Nav) -> Option<Self> {
        let class = match nav {
            Nav::Next | Nav::Down | Nav::Up(_) => Self::Any,
            Nav::NextSkip | Nav::DownSkip | Nav::UpSkipTrivia(_) => Self::AnonymousAndExtras,
            Nav::NextSkipExtras | Nav::DownSkipExtras | Nav::UpSkipExtras(_) => Self::ExtrasOnly,
            Nav::NextExact | Nav::DownExact | Nav::UpExact(_) => Self::Exact,
            Nav::Epsilon
            | Nav::Stay
            | Nav::StayExact
            | Nav::ChildlessSkipTrivia
            | Nav::ChildlessSkipExtras
            | Nav::ChildlessExact => return None,
        };
        Some(class)
    }
}

pub struct AnchorSemantics<'a> {
    pattern_facts: &'a PatternFacts,
}

pub(crate) fn has_direct_alternative_nav(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Alternation(_) => true,
        Pattern::CapturedPattern(cap) => {
            cap.inner().as_ref().is_some_and(has_direct_alternative_nav)
        }
        _ => false,
    }
}

impl<'a> AnchorSemantics<'a> {
    pub fn new(pattern_facts: &'a PatternFacts) -> Self {
        Self { pattern_facts }
    }

    pub fn pattern_may_match_anonymous_node(&self, pattern: Option<&Pattern>) -> bool {
        pattern.is_some_and(|pattern| self.pattern_facts.pattern_may_match_anonymous_node(pattern))
    }

    pub fn check_trailing_anchor(&self, items: &[SeqItem]) -> (bool, Option<Nav>) {
        if matches!(items.last(), Some(SeqItem::Anchor(_))) {
            let trailing_is_exact = items
                .iter()
                .rev()
                .take_while(|item| matches!(item, SeqItem::Anchor(_)))
                .any(|item| matches!(item, SeqItem::Anchor(anchor) if anchor.is_exact()));
            if trailing_is_exact {
                return (true, Some(Nav::UpExact(1)));
            }

            let prev_pattern = items.iter().rev().skip(1).find_map(|item| {
                if let SeqItem::Pattern(e) = item {
                    Some(e)
                } else {
                    None
                }
            });

            let nav = if self.pattern_may_match_anonymous_node(prev_pattern) {
                Nav::UpSkipExtras(1)
            } else {
                Nav::UpSkipTrivia(1)
            };
            return (true, Some(nav));
        }

        if items.len() == 1
            && let Some(SeqItem::Pattern(Pattern::SeqPattern(seq))) = items.first()
        {
            let seq_items: Vec<_> = seq.items().collect();
            return self.check_trailing_anchor(&seq_items);
        }

        (false, None)
    }

    pub fn check_leading_anchor(&self, items: &[SeqItem]) -> Option<Nav> {
        if items.len() == 1
            && let Some(SeqItem::Pattern(Pattern::SeqPattern(seq))) = items.first()
        {
            let seq_items: Vec<_> = seq.items().collect();
            return self.check_leading_anchor(&seq_items);
        }

        let is_inside_node = true;
        let (_, nav) = self
            .compute_nav_modes(items, is_inside_node)
            .into_iter()
            .next()?;
        match nav {
            Some(nav @ (Nav::DownExact | Nav::DownSkip | Nav::DownSkipExtras)) => Some(nav),
            _ => None,
        }
    }

    pub fn compute_nav_modes(
        &self,
        items: &[SeqItem],
        is_inside_node: bool,
    ) -> Vec<(usize, Option<Nav>)> {
        let mut result = Vec::new();
        let mut pending_anchor_exact = None;
        let mut prev_is_anonymous = false;
        let mut is_first_pattern = true;

        for (idx, item) in items.iter().enumerate() {
            match item {
                SeqItem::Anchor(anchor) => {
                    pending_anchor_exact =
                        Some(pending_anchor_exact.unwrap_or(false) || anchor.is_exact());
                }
                SeqItem::Pattern(pattern) => {
                    let current_is_anonymous =
                        self.pattern_facts.pattern_may_match_anonymous_node(pattern);
                    let current_is_anonymous_for_anchor = if has_direct_alternative_nav(pattern) {
                        false
                    } else {
                        current_is_anonymous
                    };
                    let nav = if let Some(is_exact) = pending_anchor_exact {
                        if is_first_pattern && is_inside_node {
                            Some(if is_exact {
                                Nav::DownExact
                            } else if current_is_anonymous_for_anchor {
                                Nav::DownSkipExtras
                            } else {
                                Nav::DownSkip
                            })
                        } else if !is_first_pattern {
                            Some(if is_exact {
                                Nav::NextExact
                            } else if prev_is_anonymous || current_is_anonymous_for_anchor {
                                Nav::NextSkipExtras
                            } else {
                                Nav::NextSkip
                            })
                        } else {
                            None
                        }
                    } else if !is_first_pattern {
                        Some(Nav::Next)
                    } else {
                        None
                    };

                    result.push((idx, nav));
                    pending_anchor_exact = None;
                    prev_is_anonymous = current_is_anonymous;
                    is_first_pattern = false;
                }
            }
        }

        result
    }
}
