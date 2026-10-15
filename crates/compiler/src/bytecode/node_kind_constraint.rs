use crate::core::NodeKindId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum NodeKindConstraint {
    #[default]
    Any,
    Named(Option<NodeKindId>),
    Anonymous(Option<NodeKindId>),
}

impl NodeKindConstraint {
    pub fn to_bytes(self) -> (u8, u16) {
        match self {
            Self::Any => (0b00, 0),
            Self::Named(opt) => (0b01, opt.map_or(0, u16::from)),
            Self::Anonymous(opt) => (0b10, opt.map_or(0, u16::from)),
        }
    }

    pub fn from_bytes(node_class: u8, node_val: u16) -> Self {
        Self::try_from_bytes(node_class, node_val).unwrap_or_else(|| {
            panic!("invalid node-kind constraint: class={node_class}, value={node_val}")
        })
    }

    pub fn try_from_bytes(node_class: u8, node_val: u16) -> Option<Self> {
        match node_class {
            0b00 => (node_val == 0).then_some(Self::Any),
            0b01 | 0b10 => {
                let id = if node_val == 0 {
                    None
                } else {
                    Some(NodeKindId::try_from(node_val).ok()?)
                };
                Some(if node_class == 0b01 {
                    Self::Named(id)
                } else {
                    Self::Anonymous(id)
                })
            }
            _ => None,
        }
    }

    pub fn kind_id(&self) -> Option<NodeKindId> {
        match self {
            Self::Any => None,
            Self::Named(opt) | Self::Anonymous(opt) => *opt,
        }
    }

    pub fn is_any(&self) -> bool {
        matches!(self, Self::Any)
    }

    pub fn is_named(&self) -> bool {
        matches!(self, Self::Named(_))
    }

    pub fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous(_))
    }
}
