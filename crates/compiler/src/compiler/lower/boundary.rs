use std::collections::BTreeMap;

use crate::compiler::analyze::boundary::ExitPort;
use plotnik_runtime::PortId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ExitSignature {
    ports: Box<[ExitPort]>,
}

impl ExitSignature {
    pub(crate) fn from_ports(ports: impl IntoIterator<Item = ExitPort>) -> Self {
        let ports = ports
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice();
        assert!(
            !ports.is_empty(),
            "an exit signature exposes at least one port"
        );
        assert!(
            ports.len() <= usize::from(PortId::COUNT),
            "operational exit signature exceeds the maximum port universe"
        );
        Self { ports }
    }

    pub(crate) fn singleton(port: ExitPort) -> Self {
        Self::from_ports([port])
    }

    pub(crate) fn ports(&self) -> &[ExitPort] {
        &self.ports
    }

    pub(crate) fn port_id(&self, port: ExitPort) -> Option<PortId> {
        let index = self.ports.binary_search(&port).ok()?;
        PortId::new(u8::try_from(index).expect("an exit signature has at most eight ports"))
    }

    pub(crate) fn len(&self) -> usize {
        self.ports.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExitMap<T> {
    targets: BTreeMap<ExitPort, T>,
}

impl<T> ExitMap<T> {
    pub(crate) fn new() -> Self {
        Self {
            targets: BTreeMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, port: ExitPort, target: T) -> Option<T> {
        self.targets.insert(port, target)
    }

    pub(crate) fn get(&self, port: ExitPort) -> Option<&T> {
        self.targets.get(&port)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (ExitPort, &T)> {
        self.targets.iter().map(|(&port, target)| (port, target))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}
