use serde::{Deserialize, Serialize};

/// How an address entered the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointSource {
    Qr,
    Mdns,
    Manual,
    LastSuccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub address: String,
    pub source: EndpointSource,
}

/// One finished probe. Fingerprint mismatches are never selected.
#[derive(Debug, Clone, Copy)]
pub struct Probe {
    pub fingerprint_matches: bool,
    pub rtt_ms: u32,
}

/// Lowest-latency probe whose certificate fingerprint matched.
pub fn choose_endpoint(probes: &[Probe]) -> Option<usize> {
    probes
        .iter()
        .enumerate()
        .filter(|(_, probe)| probe.fingerprint_matches)
        .min_by_key(|(_, probe)| probe.rtt_ms)
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_mismatched_fingerprint() {
        let probes = [
            Probe { fingerprint_matches: false, rtt_ms: 1 },
            Probe { fingerprint_matches: true, rtt_ms: 40 },
            Probe { fingerprint_matches: true, rtt_ms: 12 },
        ];
        assert_eq!(choose_endpoint(&probes), Some(2));
    }

    #[test]
    fn empty_when_nothing_matches() {
        let probes = [Probe { fingerprint_matches: false, rtt_ms: 1 }];
        assert_eq!(choose_endpoint(&probes), None);
    }
}
