//! Jellyfin sign-in: find the server on this network, then Quick Connect.
//!
//! It replaces the plex.tv PIN flow on the same screen and through the same observations: a
//! [`LoginProgress::CodeReady`] puts the code up, [`LoginProgress::Authorized`] moves the screen
//! on, and one terminal [`LoginProgress::SignedIn`] carries the server — so the owner commits and
//! persists the session exactly as it did for Plex. Nothing here needs typing on the remote: the
//! server answers a broadcast with its own address, and the code is approved from a phone.

use std::time::{Duration, Instant};

use plx_plex::jellyfin::login::{self as jf, QuickConnect};
use plx_plex::plex::probe;
use plx_plex::plex::session::{ServerRef, SourceRef};
use plx_telemetry::telemetry::incident::{IncidentContext, IncidentKind};

use super::{log, output_failed, owner, AuthProgress, LoginProgress, RegistryProgress};

/// The inherited plex.tv sign-in, kept reachable for comparison runs only.
pub(super) fn plex_login_requested() -> bool {
    std::env::var_os("PLXNATIVE_PLEX_LOGIN").is_some()
}

/// Jellyfin forgets an unapproved request after ten minutes; replace the code a little before.
const CODE_LIFETIME: Duration = Duration::from_secs(9 * 60);
const POLL_EVERY: Duration = Duration::from_secs(2);
/// An hour of codes, as the Plex flow allowed, before the screen offers *Try again* itself.
const MAX_CODES: u32 = 6;
/// What stands in for the plex.tv account token a Jellyfin sign-in does not have.
pub(super) const ACCOUNT_PLACEHOLDER: &str = "jellyfin-no-plex-account";
/// How long to listen for servers answering the broadcast.
const DISCOVERY_WAIT: Duration = Duration::from_secs(4);

fn fail(output: &dyn owner::ObservationSink, epoch: u64, message: &str, kind: IncidentKind) {
    output_failed(output, epoch, message, IncidentContext::new(kind, None), None);
}

pub(super) fn login_worker(epoch: u64, output: &dyn owner::ObservationSink) {
    if !output.live() {
        return;
    }
    // 1) the server.
    let url = match jf::server_override() {
        Some(url) => url,
        None => {
            log("auth: looking for a Jellyfin server on this network");
            match jf::discover(DISCOVERY_WAIT).into_iter().next() {
                Some(found) => found.url,
                None => {
                    return fail(
                        output,
                        epoch,
                        "No Jellyfin server answered on this network. Make sure it is running and on the same network as this TV, then try again.",
                        IncidentKind::PinCreate,
                    )
                }
            }
        }
    };
    let Some(origin) = jf::origin_of(&url) else {
        return fail(output, epoch, "The server's address could not be read. Try again.", IncidentKind::PinCreate);
    };
    let Some((name, server_id)) = jf::server_identity(&origin) else {
        return fail(
            output,
            epoch,
            "Found a server, but it is not answering as Jellyfin. Check that it is running, then try again.",
            IncidentKind::PinCreate,
        );
    };
    log(&format!("auth: Jellyfin server at {}", origin.log_form()));
    jf::set_login_hint(&url);
    if jf::quick_connect_enabled(&origin) == Some(false) {
        return fail(
            output,
            epoch,
            "Quick Connect is turned off on this server. Turn it on in Jellyfin's Dashboard → General, then try again.",
            IncidentKind::PinCreate,
        );
    }

    // 2) Quick Connect, re-issuing the code while the screen stays up.
    let mut generation = 0;
    let token = 'codes: loop {
        generation += 1;
        if generation > MAX_CODES {
            return fail(output, epoch, "The sign-in code expired. Select Try again for a new one.", IncidentKind::PinExpired);
        }
        if generation > 1 && !output.progress(LoginProgress::CodeReplacing { epoch }.into()) {
            return;
        }
        let Some(qc) = QuickConnect::initiate(&origin) else {
            return fail(output, epoch, "The server did not give a sign-in code. Try again.", IncidentKind::PinCreate);
        };
        #[cfg(feature = "devtriggers")]
        log(&format!("auth: quick connect code {}", qc.code));
        if !output.progress(LoginProgress::CodeReady { epoch, code: qc.code.clone(), qr_png: Vec::new() }.into()) {
            return;
        }
        let shown = Instant::now();
        while shown.elapsed() < CODE_LIFETIME {
            std::thread::sleep(POLL_EVERY);
            if !output.live() {
                return;
            }
            // `None` is a missed poll or a code the server has dropped; either way the lifetime
            // above decides when to stop waiting on it.
            if qc.approved(&origin) == Some(true) {
                match qc.token(&origin) {
                    Some((token, _user)) => break 'codes token,
                    None => {
                        return fail(
                            output,
                            epoch,
                            "The code was approved, but the server would not complete the sign-in. Try again.",
                            IncidentKind::Authorization,
                        )
                    }
                }
            }
        }
        log("auth: the sign-in code ran out — asking for a fresh one");
    };
    log("auth: quick connect approved");
    if !output.live() {
        return;
    }
    // The owner keeps this as the plex.tv ACCOUNT token, and the session store persists a sign-in
    // only when it has one. This build has no plex.tv account, so it records a fixed placeholder:
    // never the Jellyfin token, which the inherited account code would otherwise carry to plex.tv
    // (the net layer refuses plex.tv outright as well, `plx_net::net::is_plex_tv`).
    if !output.progress(LoginProgress::Authorized { epoch, token: ACCOUNT_PLACEHOLDER.to_string() }.into()) {
        return;
    }
    let server = ServerRef {
        name,
        machine_id: server_id,
        address: origin.host().to_owned(),
        port: i64::from(origin.port()),
        token,
        tier: Some(probe::configured_tier(origin.host())),
        origin_url: origin.base(),
        ..Default::default()
    };
    // The server as a roster entry too: the owner commits the sign-in only once the registry has
    // been told which sources exist and which is primary, as Plex discovery always did first.
    let source = SourceRef {
        machine_id: server.machine_id.clone(),
        name: server.name.clone(),
        owned: true,
        home: true,
        address: server.address.clone(),
        port: server.port,
        token: server.token.clone(),
        tier: server.tier,
        origin_url: server.origin_url.clone(),
        ..Default::default()
    };
    if !output.progress(AuthProgress::Registry(RegistryProgress::Install {
        epoch,
        expected: None,
        sources: vec![source.clone()],
        primary: Some(0),
    })) {
        return;
    }
    output.terminal(LoginProgress::SignedIn { epoch, server, sources: vec![source], users: Vec::new() }.into());
}
