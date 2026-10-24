//! Nearby networks, from `wpa_cli scan_results`.
//!
//! WPA2-Personal is the supported standard (requirement 10). This `wpa_supplicant` has no
//! SAE (Phase 0E), so a WPA3-only network cannot be joined; a mixed WPA2/WPA3 network
//! offers WPA2 as well and joins as WPA2.

/// What it takes to join a network.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Security {
    /// WPA2-Personal (or WPA-Personal): a password, stored as the derived key.
    Wpa2,
    /// No password.
    Open,
    /// WPA3-Personal only: not supported (no SAE in this `wpa_supplicant`).
    Wpa3Only,
    /// Enterprise (802.1X), WEP or Enhanced Open: not supported.
    Unsupported,
}

impl Security {
    pub fn joinable(self) -> bool {
        matches!(self, Security::Wpa2 | Security::Open)
    }

    /// From a scan result's flags, eg `[WPA2-PSK-CCMP][ESS]`.
    pub fn from_flags(flags: &str) -> Security {
        let f = flags.to_ascii_uppercase();
        let psk = f.contains("WPA2-PSK") || f.contains("WPA-PSK") || f.contains("RSN-PSK");
        if psk {
            return Security::Wpa2;
        }
        if f.contains("SAE") {
            return Security::Wpa3Only;
        }
        if f.contains("EAP") || f.contains("WEP") || f.contains("OWE") || f.contains("WPA") {
            return Security::Unsupported;
        }
        Security::Open
    }
}

/// One network name, merged across the access points that share it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub ssid: Vec<u8>,
    /// The strongest signal heard, in dBm.
    pub signal: i32,
    pub security: Security,
}

impl Seen {
    /// Signal as 1 to 3 bars.
    pub fn bars(&self) -> u8 {
        bars(self.signal)
    }
}

pub fn bars(dbm: i32) -> u8 {
    match dbm {
        d if d >= -60 => 3,
        d if d >= -72 => 2,
        _ => 1,
    }
}

/// Parses `scan_results`. Hidden networks (no name) are left out (I4). Networks sharing a
/// name are merged: the strongest signal, and joinable if any of them is.
pub fn parse(text: &str) -> Vec<Seen> {
    let mut seen: Vec<Seen> = Vec::new();
    for line in text.lines() {
        let mut cols = line.splitn(5, '\t');
        let (Some(bssid), Some(_freq), Some(signal), Some(flags)) =
            (cols.next(), cols.next(), cols.next(), cols.next())
        else {
            continue;
        };
        if bssid.len() != 17 || bssid.matches(':').count() != 5 {
            continue;
        }
        let Ok(signal) = signal.trim().parse::<i32>() else {
            continue;
        };
        let ssid = crate::text::unescape(cols.next().unwrap_or(""));
        if ssid.is_empty() || ssid.iter().all(|&b| b == 0) {
            continue;
        }
        let security = Security::from_flags(flags);
        match seen.iter_mut().find(|s| s.ssid == ssid) {
            Some(s) => {
                s.signal = s.signal.max(signal);
                if !s.security.joinable() && security.joinable()
                    || s.security == Security::Open && security == Security::Wpa2
                {
                    s.security = security;
                }
            }
            None => seen.push(Seen {
                ssid,
                signal,
                security,
            }),
        }
    }
    seen.sort_by(|a, b| b.signal.cmp(&a.signal).then(a.ssid.cmp(&b.ssid)));
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESULTS: &str = "bssid / frequency / signal level / flags / ssid\n\
        aa:bb:cc:dd:ee:01\t2437\t-48\t[WPA2-PSK-CCMP][WPS][ESS]\tDropBearNet\n\
        aa:bb:cc:dd:ee:02\t5180\t-41\t[WPA2-PSK-CCMP][ESS]\tDropBearNet\n\
        aa:bb:cc:dd:ee:03\t2412\t-80\t[WPA2-PSK+SAE-CCMP][ESS]\tMixed\n\
        aa:bb:cc:dd:ee:04\t2412\t-70\t[RSN-SAE-CCMP][ESS]\tOnlyWpa3\n\
        aa:bb:cc:dd:ee:05\t2412\t-66\t[ESS]\tCafe \\\"Free\\\"\n\
        aa:bb:cc:dd:ee:06\t2412\t-55\t[WPA2-EAP-CCMP][ESS]\tWork\n\
        aa:bb:cc:dd:ee:07\t2412\t-50\t[WPA2-PSK-CCMP][ESS]\t\n\
        aa:bb:cc:dd:ee:08\t2412\t-52\t[WPA2-PSK-CCMP][ESS]\t\\x00\\x00\n\
        aa:bb:cc:dd:ee:09\t2412\t-77\t[WEP][ESS]\tOld\n\
        garbage line\n";

    #[test]
    fn results_are_parsed_merged_and_sorted() {
        let s = parse(RESULTS);
        let names: Vec<String> = s.iter().map(|n| crate::text::shown_name(&n.ssid)).collect();
        assert_eq!(
            names,
            [
                "DropBearNet",
                "Work",
                "Cafe \"Free\"",
                "OnlyWpa3",
                "Old",
                "Mixed"
            ]
        );
        assert_eq!(s[0].signal, -41, "the strongest access point counts");
        assert_eq!(s[0].security, Security::Wpa2);
    }

    #[test]
    fn security_is_classified() {
        let s = parse(RESULTS);
        let by = |n: &str| s.iter().find(|x| x.ssid == n.as_bytes()).unwrap().security;
        assert_eq!(by("Mixed"), Security::Wpa2, "mixed WPA2/WPA3 joins as WPA2");
        assert_eq!(by("OnlyWpa3"), Security::Wpa3Only);
        assert_eq!(by("Cafe \"Free\""), Security::Open);
        assert_eq!(by("Work"), Security::Unsupported);
        assert_eq!(by("Old"), Security::Unsupported);
        assert!(Security::Wpa2.joinable() && Security::Open.joinable());
        assert!(!Security::Wpa3Only.joinable());
    }

    #[test]
    fn hidden_networks_are_left_out() {
        assert!(parse(RESULTS)
            .iter()
            .all(|n| !n.ssid.is_empty() && n.ssid[0] != 0));
    }

    #[test]
    fn signal_bars() {
        assert_eq!(bars(-41), 3);
        assert_eq!(bars(-65), 2);
        assert_eq!(bars(-85), 1);
    }
}
