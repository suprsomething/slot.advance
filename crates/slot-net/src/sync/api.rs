//! The few REST calls the supervisor makes to Syncthing (plan 3.4, 8.4 refinements 4, 5).
//! All of them run on the supervisor thread, never the game thread.

use std::time::Duration;

use super::config::{Layout, Settings};
use super::https::{Endpoint, Error, Response};

const RETRIES: u32 = 4;
const RETRY_GAP: Duration = Duration::from_millis(200);
use super::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Ok,
    /// Answers health in plain HTTP: its HTTPS was switched off (a floor violation).
    PlainHttp,
    NotAnswering(String),
}

/// The web interface settings, as Syncthing currently runs them.
#[derive(Clone, PartialEq, Eq)]
pub struct Gui {
    pub enabled: bool,
    pub address: String,
    pub user: String,
    pub hash: String,
    pub tls: bool,
    pub insecure_admin: bool,
    pub insecure_hostcheck: bool,
}

impl std::fmt::Debug for Gui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gui")
            .field("enabled", &self.enabled)
            .field("address", &self.address)
            .field("tls", &self.tls)
            .finish_non_exhaustive()
    }
}

impl Gui {
    /// The JSON for `PATCH /rest/config/gui` repairing the floor fields that are wrong
    /// (the password is repaired separately, as a reset), or `None`.
    pub fn floor_patch(&self) -> Option<String> {
        let mut fields = Vec::new();
        if !self.enabled {
            fields.push("\"enabled\":true");
        }
        if !self.tls {
            fields.push("\"useTLS\":true");
        }
        if self.insecure_admin {
            fields.push("\"insecureAdminAccess\":false");
        }
        if self.insecure_hostcheck {
            fields.push("\"insecureSkipHostcheck\":false");
        }
        (!fields.is_empty()).then(|| format!("{{{}}}", fields.join(",")))
    }

    pub fn violations(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.enabled {
            v.push("web interface switched off");
        }
        if !self.tls {
            v.push("HTTPS switched off");
        }
        if self.hash.is_empty() {
            v.push("no password");
        }
        if self.insecure_admin {
            v.push("insecureAdminAccess on");
        }
        if self.insecure_hostcheck {
            v.push("insecureSkipHostcheck on");
        }
        v
    }
}

/// One folder's error fields from `/rest/db/status` (plan 8.2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderState {
    pub error: String,
    pub watch_error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Folder {
    State(FolderState),
    /// No such folder (the user removed or renamed it in the web interface): not an error.
    Absent,
}

pub trait Api: Send {
    fn health(&mut self) -> Health;
    fn gui(&mut self) -> Result<Gui, String>;
    fn patch_gui(&mut self, body: &str) -> Result<(), String>;
    fn folder(&mut self, id: &str) -> Result<Folder, String>;
    fn shutdown(&mut self) -> Result<(), String>;
}

/// The real thing: HTTPS to Syncthing's web interface with the pinned certificate.
pub struct Rest {
    endpoint: Endpoint,
    key: String,
}

impl Rest {
    pub fn new(layout: &Layout, settings: &Settings) -> Option<Rest> {
        Some(Rest {
            endpoint: Endpoint::new(settings.endpoint()?, &layout.https_cert()),
            key: settings.api_key.clone(),
        })
    }

    /// One request, retried briefly while the connection is refused: Syncthing restarts
    /// its web interface listener after every change to the web interface settings (seen
    /// with 2.1.6), so a request straight after one may find nothing listening for a
    /// moment. At most 5 tries, 200 ms apart.
    fn send(&self, method: &str, path: &str, body: Option<&str>) -> Result<Response, Error> {
        let mut tries = 0;
        loop {
            match self.endpoint.request(method, path, Some(&self.key), body) {
                Err(Error::Unreachable(_)) if tries < RETRIES => {
                    tries += 1;
                    std::thread::sleep(RETRY_GAP);
                }
                r => return r,
            }
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
        let r = self.send(method, path, body).map_err(|e| e.to_string())?;
        if (200..300).contains(&r.status) {
            Ok(r.body)
        } else {
            Err(format!("{method} {path}: HTTP {}", r.status))
        }
    }
}

impl Api for Rest {
    fn health(&mut self) -> Health {
        match self
            .endpoint
            .request("GET", "/rest/noauth/health", None, None)
        {
            Ok(r) if r.status == 200 && r.body.contains("OK") => Health::Ok,
            Ok(r) => Health::NotAnswering(format!("health: HTTP {}", r.status)),
            Err(e @ (Error::Tls(_) | Error::WrongCertificate)) => {
                if self.endpoint.plain_health() {
                    Health::PlainHttp
                } else {
                    Health::NotAnswering(e.to_string())
                }
            }
            Err(e) => Health::NotAnswering(e.to_string()),
        }
    }

    fn gui(&mut self) -> Result<Gui, String> {
        let body = self.call("GET", "/rest/config/gui", None)?;
        let v = json::parse(&body).ok_or("unreadable web interface settings")?;
        Ok(Gui {
            enabled: v.bool("enabled").unwrap_or(true),
            address: v.str("address").unwrap_or_default().to_string(),
            user: v.str("user").unwrap_or_default().to_string(),
            hash: v.str("password").unwrap_or_default().to_string(),
            tls: v.bool("useTLS").unwrap_or(false),
            insecure_admin: v.bool("insecureAdminAccess").unwrap_or(false),
            insecure_hostcheck: v.bool("insecureSkipHostcheck").unwrap_or(false),
        })
    }

    fn patch_gui(&mut self, body: &str) -> Result<(), String> {
        self.call("PATCH", "/rest/config/gui", Some(body))
            .map(|_| ())
    }

    fn folder(&mut self, id: &str) -> Result<Folder, String> {
        let path = format!("/rest/db/status?folder={id}");
        let r = self.send("GET", &path, None).map_err(|e| e.to_string())?;
        if r.status == 404 || (r.status == 500 && r.body.contains("no such folder")) {
            return Ok(Folder::Absent);
        }
        if !(200..300).contains(&r.status) {
            return Err(format!("folder status: HTTP {}", r.status));
        }
        let v = json::parse(&r.body).ok_or("unreadable folder status")?;
        Ok(Folder::State(FolderState {
            error: v.str("error").unwrap_or_default().to_string(),
            watch_error: v.str("watchError").unwrap_or_default().to_string(),
        }))
    }

    fn shutdown(&mut self) -> Result<(), String> {
        // Within the 3 s first stop stage (P2-5): 1 s to connect, 1.5 s to answer.
        let quick = self
            .endpoint
            .with_limits(Duration::from_secs(1), Duration::from_millis(1500));
        let r = quick
            .request("POST", "/rest/system/shutdown", Some(&self.key), None)
            .map_err(|e| e.to_string())?;
        if (200..300).contains(&r.status) {
            Ok(())
        } else {
            Err(format!("shutdown: HTTP {}", r.status))
        }
    }
}

/// The JSON for a credentials reset: the user name back to `slotadvance` and a new
/// password (Syncthing stores it as a bcrypt hash; checked with 2.1.6, plan 8.2).
pub fn reset_body(password: &str) -> String {
    format!(
        "{{\"user\":{},\"password\":{}}}",
        json::string(super::creds::USER),
        json::string(password)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui() -> Gui {
        Gui {
            enabled: true,
            address: "0.0.0.0:8384".into(),
            user: "slotadvance".into(),
            hash: "$2a$10$x".into(),
            tls: true,
            insecure_admin: false,
            insecure_hostcheck: false,
        }
    }

    #[test]
    fn the_patch_names_only_the_wrong_fields() {
        assert_eq!(gui().floor_patch(), None);
        let g = Gui {
            tls: false,
            ..gui()
        };
        assert_eq!(g.floor_patch().as_deref(), Some("{\"useTLS\":true}"));
        let g = Gui {
            insecure_admin: true,
            insecure_hostcheck: true,
            ..gui()
        };
        assert_eq!(
            g.floor_patch().as_deref(),
            Some("{\"insecureAdminAccess\":false,\"insecureSkipHostcheck\":false}")
        );
        // An empty password is a violation, but not part of this patch (it is a reset).
        let g = Gui {
            hash: String::new(),
            ..gui()
        };
        assert_eq!(g.violations(), ["no password"]);
        assert_eq!(g.floor_patch(), None);
    }

    #[test]
    fn the_reset_body_is_valid_json() {
        let b = reset_body("alpha bravo charlie delta echo");
        let v = json::parse(&b).unwrap();
        assert_eq!(v.str("user"), Some("slotadvance"));
        assert_eq!(v.str("password"), Some("alpha bravo charlie delta echo"));
    }

    #[test]
    fn debug_output_never_contains_the_hash() {
        assert!(!format!("{:?}", gui()).contains("$2a$"));
    }
}
