//! The window, driven the way a person drives it: a pointer pressed and
//! released on a control found by its accessible name, keys typed into
//! fields. Each test checks what the window asks the worker to do.
//!
//! For pictures, `agents-kitbag --demo-shot <png>` draws any screen.

use std::sync::{Arc, Mutex};

use agents_kitbag_core::assistant::Assistant;
use agents_kitbag_core::gh;
use agents_kitbag_core::platform::Os;
use agents_kitbag_core::setup::Done;
use agents_kitbag_core::storage::StoreChoice;
use eframe::egui::accesskit::Role;
use eframe::egui::{self, Event, Id, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use super::{Screen, Section, State, form};
use crate::snapshot::{SaveOutcome, Snapshot};
use crate::worker::{Command, Worker};
use crate::{demo, theme};

struct Rig {
    installed: bool,
    snapshot: Snapshot,
    ui: State,
    commands: Vec<Command>,
}

/// The demo's config with every store probed and every token checked.
fn demo_snapshot() -> Snapshot {
    let dir = tempfile::tempdir().unwrap();
    let shared = Arc::new(Mutex::new(Snapshot::empty(Os::Mac)));
    let mut worker = Worker::new(
        demo::backend(dir.path()),
        Arc::clone(&shared),
        Box::new(|| {}),
    );
    worker.handle(Command::LoadStores);
    worker.handle(Command::CheckAll);
    shared.lock().unwrap().clone()
}

fn rig(snapshot: Snapshot) -> Harness<'static, Rig> {
    let mut harness = Harness::builder()
        .with_size(vec2(960.0, 760.0))
        .with_theme(egui::Theme::Dark)
        .build_ui_state(
            |ui, rig: &mut Rig| {
                // Fonts apply from the next frame, so the first one only sets them up.
                if !std::mem::replace(&mut rig.installed, true) {
                    theme::install(ui.ctx(), false);
                    return;
                }
                let commands = super::show(ui, &rig.snapshot, &mut rig.ui);
                rig.commands.extend(commands);
            },
            Rig {
                installed: false,
                snapshot,
                ui: State::default(),
                commands: Vec::new(),
            },
        );
    harness.run_steps(4);
    harness
}

fn step(h: &mut Harness<Rig>) {
    h.run_steps(3);
}

fn click(h: &mut Harness<Rig>, label: &str) {
    h.get_by_label(label).click();
    step(h);
}

fn has(h: &Harness<Rig>, label: &str) -> bool {
    h.query_by_label(label).is_some()
}

/// Clicks into a field and types.
fn type_into(h: &mut Harness<Rig>, id: Id, text: &str) {
    let mut found = None;
    for _ in 0..3 {
        found = h.ctx.read_response(id);
        if found.is_some() {
            break;
        }
        h.step();
    }
    let pos = found
        .unwrap_or_else(|| panic!("no field {id:?}"))
        .rect
        .center();
    h.event(Event::PointerMoved(pos));
    step(h);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        step(h);
    }
    h.event(Event::Text(text.into()));
    step(h);
}

fn commands(h: &mut Harness<Rig>) -> Vec<Command> {
    std::mem::take(&mut h.state_mut().commands)
}

#[test]
fn the_sidebar_says_how_each_integration_is_doing() {
    let h = rig(demo_snapshot());
    for label in [
        "Jira, working",
        "Linear, working",
        "Notion, not configured",
        "Azure DevOps, not configured",
        "Asana, token refused",
        "GitHub, working",
        "Figma, not configured",
    ] {
        assert!(has(&h, label), "{label}");
    }
}

#[test]
fn the_rail_goes_between_the_sections_and_the_settings() {
    let mut h = rig(demo_snapshot());
    assert!(has(&h, "Assistant"));
    click(&mut h, "Settings");
    assert_eq!(h.state().ui.screen, Screen::Settings);
    // The settings take the place of the section's list.
    assert!(!has(&h, "Jira, working"));
    assert!(!has(&h, "Check all tokens"));

    h.get_by_role_and_label(Role::Button, "MCP servers").click();
    step(&mut h);
    assert_eq!(h.state().ui.screen, Screen::Section(Section::Servers));
    assert!(has(&h, "Jira, working"));
}

#[test]
fn the_list_is_of_the_chosen_assistants_servers() {
    let mut h = rig(demo_snapshot());
    h.get_by_label("Assistant").click();
    step(&mut h);
    click(&mut h, "Codex");
    assert_eq!(commands(&mut h), [Command::UseAssistant(Assistant::Codex)]);

    let mut snapshot = demo_snapshot();
    snapshot.assistant = Assistant::Codex;
    snapshot.needs_restart = true;
    let mut h = rig(snapshot);
    click(&mut h, "Notion, not configured");
    assert!(has(&h, "Connect Notion to Codex"));
    assert!(has(
        &h,
        "New Codex sessions load the changes. One that is open keeps what it has."
    ));
    click(&mut h, "Got it");
    assert_eq!(commands(&mut h), [Command::RestartDone]);
}

#[test]
fn a_server_the_service_hosts_can_be_checked_and_deleted_but_not_edited() {
    let h = rig(demo_snapshot());
    assert!(has(&h, "Replace token"));

    let mut snapshot = demo_snapshot();
    let jira = snapshot
        .integrations
        .iter_mut()
        .find(|i| i.key == "jira")
        .unwrap();
    jira.instances[0].hosted = true;
    let h = rig(snapshot);
    assert!(has(&h, "jira-acme"));
    assert!(has(&h, "Check"));
    assert!(has(&h, "Delete"));
    assert!(!has(&h, "Replace token"));
}

#[test]
fn an_assistant_that_is_not_on_this_machine_cannot_be_chosen() {
    let mut snapshot = demo_snapshot();
    snapshot.installed = vec![Assistant::ClaudeDesktop, Assistant::ClaudeCode];
    let mut h = rig(snapshot);
    h.get_by_label("Assistant").click();
    step(&mut h);
    assert!(
        h.get_by_label("Codex (not installed)")
            .accesskit_node()
            .is_disabled()
    );
    assert!(!h.get_by_label("Claude Code").accesskit_node().is_disabled());
}

#[test]
fn a_form_can_set_the_server_up_for_the_other_assistants_too() {
    let mut snapshot = demo_snapshot();
    snapshot.installed = vec![Assistant::ClaudeDesktop, Assistant::Codex];
    let mut h = rig(snapshot);
    click(&mut h, "Asana, token refused");
    click(&mut h, "Replace token");
    assert!(has(&h, "Set up for"));
    // Only the assistants on this machine are offered.
    assert!(!has(&h, "Claude Code"));
    type_into(&mut h, form::token_field(), "tok");
    click(&mut h, "Codex");
    click(&mut h, "Save");
    let commands = commands(&mut h);
    let Some(Command::Save { also, .. }) = commands.last() else {
        panic!("no Save in {commands:?}");
    };
    assert_eq!(also, &[Assistant::Codex]);
}

#[test]
fn leaving_for_the_settings_closes_an_open_form() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Notion, not configured");
    click(&mut h, "Add workspace");
    assert!(h.state().ui.form.is_some());
    click(&mut h, "Settings");
    assert!(h.state().ui.form.is_none());
}

#[test]
fn a_configured_server_shows_where_its_token_is_and_whether_it_works() {
    let h = rig(demo_snapshot());
    for label in [
        "jira-acme",
        "Token works",
        "https://acme.atlassian.net",
        "ada@acme.com",
        "macOS Keychain",
    ] {
        assert!(has(&h, label), "{label}");
    }
    assert!(!has(&h, "demo-token"), "a token is never drawn");
}

#[test]
fn an_expired_token_is_called_out() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Asana, token refused");
    assert!(has(&h, "Token expired or revoked (HTTP 401)"));
    assert!(has(&h, "Config file (plain text)"));
}

#[test]
fn checking_one_server_or_all_of_them() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Check");
    assert_eq!(commands(&mut h), [Command::Check("jira-acme".into())]);
    click(&mut h, "Check all tokens");
    assert_eq!(commands(&mut h), [Command::CheckAll]);
}

#[test]
fn with_no_server_there_is_nothing_to_check_and_the_row_says_so() {
    let mut snapshot = demo_snapshot();
    for integration in &mut snapshot.integrations {
        integration.instances.clear();
    }
    let mut h = rig(snapshot);
    assert!(
        h.get_by_label("Check all tokens")
            .accesskit_node()
            .is_disabled()
    );
    click(&mut h, "Check all tokens");
    assert!(commands(&mut h).is_empty());
}

#[test]
fn deleting_takes_a_second_click() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Delete");
    assert!(commands(&mut h).is_empty());
    click(&mut h, "Keep");
    assert!(!has(&h, "Delete jira-acme"));
    click(&mut h, "Delete");
    click(&mut h, "Delete jira-acme");
    assert_eq!(
        commands(&mut h),
        [Command::Delete(vec!["jira-acme".into()])]
    );
}

#[test]
fn an_unconfigured_integration_offers_to_set_it_up() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Notion, not configured");
    assert!(has(&h, "Connect Notion to Claude Desktop"));
    click(&mut h, "Add workspace");
    assert!(has(&h, "Set up Notion"));
    assert!(has(&h, "Name of this workspace"));
    assert!(has(&h, "Open the token page"));
}

#[test]
fn a_filled_form_is_saved_to_the_systems_store_by_default() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Notion, not configured");
    click(&mut h, "Add workspace");
    assert_eq!(
        commands(&mut h),
        [Command::LoadStores],
        "a form asks which stores work here"
    );
    type_into(&mut h, form::field_id(form::INSTANCE_FIELD), "acme");
    type_into(&mut h, form::token_field(), "ntn_secret");
    assert!(!has(&h, "ntn_secret"), "the token is hidden as it is typed");
    click(&mut h, "Save");
    assert_eq!(
        commands(&mut h),
        [Command::Save {
            id: 1,
            integration: "notion",
            instance: Some("acme".into()),
            values: [("token".to_owned(), "ntn_secret".to_owned())].into(),
            store: StoreChoice::System,
            also: Vec::new(),
        }]
    );
    assert!(has(&h, "Saving..."));
}

#[test]
fn the_token_page_opens_in_the_browser() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Figma, not configured");
    click(&mut h, "Set up Figma");
    commands(&mut h);
    click(&mut h, "Open the token page");
    assert_eq!(
        commands(&mut h),
        [Command::OpenUrl("https://www.figma.com/files".into())]
    );
}

#[test]
fn the_person_can_choose_plain_text_or_a_1password_vault() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Figma, not configured");
    click(&mut h, "Set up Figma");
    type_into(&mut h, form::token_field(), "figd_x");
    click(&mut h, "Config file (plain text)");
    click(&mut h, "Save");
    let Command::Save {
        store, instance, ..
    } = commands(&mut h).remove(1)
    else {
        panic!("not a save");
    };
    assert_eq!(store, StoreChoice::Plain);
    assert_eq!(instance, None, "a single integration has no instance name");

    let mut h = rig(demo_snapshot());
    click(&mut h, "Figma, not configured");
    click(&mut h, "Set up Figma");
    type_into(&mut h, form::token_field(), "figd_x");
    click(&mut h, "1Password");
    click(&mut h, "Save");
    let Command::Save { store, .. } = commands(&mut h).remove(1) else {
        panic!("not a save");
    };
    assert_eq!(
        store,
        StoreChoice::OnePassword {
            vault: "Private".into()
        }
    );
}

#[test]
fn a_refused_save_stays_in_the_form_with_its_reason() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Figma, not configured");
    click(&mut h, "Set up Figma");
    type_into(&mut h, form::token_field(), "bad");
    click(&mut h, "Save");
    h.state_mut().snapshot.save = Some(SaveOutcome {
        id: 1,
        result: Err("Token invalid (HTTP 403). Check it and try again.".into()),
    });
    step(&mut h);
    assert!(has(&h, "Token invalid (HTTP 403). Check it and try again."));
    assert!(has(&h, "Save"), "the form can be sent again");
    assert!(h.state().ui.form.is_some());
}

#[test]
fn a_save_that_worked_closes_the_form() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Figma, not configured");
    click(&mut h, "Set up Figma");
    type_into(&mut h, form::token_field(), "figd_x");
    click(&mut h, "Save");
    h.state_mut().snapshot.save = Some(SaveOutcome {
        id: 1,
        result: Ok(Done {
            key: "figma".into(),
            account: "Ada".into(),
            warning: None,
        }),
    });
    step(&mut h);
    assert!(h.state().ui.form.is_none());
}

#[test]
fn an_older_saves_outcome_does_not_close_a_newer_form() {
    let mut h = rig(demo_snapshot());
    h.state_mut().snapshot.save = Some(SaveOutcome {
        id: 1,
        result: Err("old".into()),
    });
    click(&mut h, "Figma, not configured");
    click(&mut h, "Set up Figma");
    step(&mut h);
    assert!(!has(&h, "old"));
    assert!(h.state().ui.form.is_some());
}

#[test]
fn editing_keeps_the_name_and_what_is_not_secret() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Replace token");
    assert!(has(&h, "Update jira-acme"));
    let form = h.state().ui.form.clone().unwrap();
    assert_eq!(form.editing.as_deref(), Some("jira-acme"));
    assert_eq!(form.instance, "acme");
    assert_eq!(
        form.values.get("url").map(String::as_str),
        Some("https://acme.atlassian.net")
    );
    assert!(form.values.get("token").is_none_or(String::is_empty));
    click(&mut h, "Back");
    assert!(h.state().ui.form.is_none());
}

#[test]
fn github_connects_through_the_cli_when_it_is_ready() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "GitHub, working");
    click(&mut h, "Reconnect");
    assert!(has(&h, "The GitHub CLI is installed and signed in."));
    click(&mut h, "Connect");
    let Command::Save {
        integration,
        store,
        values,
        ..
    } = commands(&mut h).remove(1)
    else {
        panic!("not a save");
    };
    assert_eq!((integration, store), ("github", StoreChoice::Gh));
    assert!(values.is_empty());
}

#[test]
fn github_without_the_cli_offers_to_install_or_sign_in() {
    let mut snapshot = demo_snapshot();
    snapshot.gh = Some(gh::State::NotInstalled);
    let mut h = rig(snapshot);
    click(&mut h, "GitHub, working");
    click(&mut h, "Reconnect");
    assert!(h.get_by_label("Connect").accesskit_node().is_disabled());
    commands(&mut h);
    click(&mut h, "Install it");
    assert_eq!(commands(&mut h), [Command::InstallGh]);

    let mut snapshot = demo_snapshot();
    snapshot.gh = Some(gh::State::NotSignedIn);
    let mut h = rig(snapshot);
    click(&mut h, "GitHub, working");
    click(&mut h, "Reconnect");
    commands(&mut h);
    click(&mut h, "Sign in");
    click(&mut h, "Check again");
    assert_eq!(commands(&mut h), [Command::GhLogin, Command::LoadStores]);
}

#[test]
fn the_node_runner_is_offered_for_migration() {
    let mut h = rig(demo_snapshot());
    click(&mut h, "Migrate");
    assert_eq!(commands(&mut h), [Command::Migrate]);
}

#[test]
fn a_change_asks_for_a_restart_on_macos_and_explains_it_on_windows() {
    let mut snapshot = demo_snapshot();
    snapshot.needs_restart = true;
    let mut h = rig(snapshot.clone());
    click(&mut h, "Restart Claude");
    assert_eq!(commands(&mut h), [Command::RestartClaude]);

    snapshot.os = Os::Windows;
    let mut h = rig(snapshot);
    assert!(!has(&h, "Restart Claude"));
    assert!(has(&h, "2. Open Claude Desktop again."));
    click(&mut h, "Done");
    assert_eq!(commands(&mut h), [Command::RestartDone]);
}

#[test]
fn purging_backups_takes_a_second_click() {
    let mut snapshot = demo_snapshot();
    snapshot.backups = 2;
    let mut h = rig(snapshot);
    click(&mut h, "Settings");
    assert_eq!(h.state().ui.screen, Screen::Settings);
    click(&mut h, "Delete backups...");
    assert!(commands(&mut h).is_empty());
    click(&mut h, "Delete all backups");
    assert_eq!(commands(&mut h), [Command::PurgeBackups]);
}

#[test]
fn a_config_that_cannot_be_used_blocks_saving_and_says_why() {
    let mut snapshot = demo_snapshot();
    snapshot.config_error = Some("The config file is not valid JSON: expected value".into());
    let mut h = rig(snapshot);
    click(&mut h, "Figma, not configured");
    click(&mut h, "Set up Figma");
    assert!(h.get_by_label("Save").accesskit_node().is_disabled());
    commands(&mut h);
    click(&mut h, "Show file");
    assert_eq!(commands(&mut h), [Command::RevealConfig]);
}

#[test]
fn a_notice_can_be_dismissed() {
    let mut snapshot = demo_snapshot();
    snapshot.notice = Some(crate::snapshot::Notice {
        tone: crate::snapshot::Tone::Good,
        text: "Deleted jira-acme".into(),
    });
    let mut h = rig(snapshot);
    assert!(has(&h, "Deleted jira-acme"));
    click(&mut h, "Dismiss");
    assert_eq!(commands(&mut h), [Command::DismissNotice]);
}

#[test]
fn an_update_is_offered_and_asked_for() {
    let mut h = rig(demo_snapshot());
    h.state_mut().ui.update = Some(crate::updates::Offer {
        version: "0.22.0".into(),
        downloading: false,
        failed: false,
    });
    step(&mut h);
    assert!(has(&h, "Version 0.22.0 is available"));
    click(&mut h, "Restart to update");
    assert!(h.state_mut().ui.take_update_request());
}

#[test]
fn the_about_card_says_how_the_last_look_for_an_update_ended() {
    use crate::updates::{Checked, Offer};
    let mut h = rig(demo_snapshot());
    click(&mut h, "Settings");
    assert!(has(
        &h,
        "Updates are looked for when the app starts and every six hours."
    ));
    assert!(
        h.get_by_label("Check for updates")
            .accesskit_node()
            .is_disabled(),
        "nothing to ask in the demo"
    );

    for (checked, line) in [
        (Checked::Checking, "Looking for a newer version..."),
        (Checked::UpToDate, "This is the newest version."),
        (
            Checked::Unreachable("The update server answered 404".into()),
            "Could not check for updates: The update server answered 404",
        ),
        (
            Checked::Elsewhere {
                version: "0.24.0".into(),
                reason: "Move the app to Applications, then open it to update.".into(),
            },
            "Version 0.24.0 is out, but this copy does not update itself. Move the app to Applications, then open it to update.",
        ),
    ] {
        h.state_mut().ui.update_check = checked;
        step(&mut h);
        assert!(has(&h, line), "{line}");
    }

    h.state_mut().ui.update_check = Checked::Available;
    h.state_mut().ui.update = Some(Offer {
        version: "0.24.0".into(),
        downloading: false,
        failed: false,
    });
    step(&mut h);
    assert!(has(&h, "Version 0.24.0 is available."));
}

#[test]
fn a_look_for_updates_can_be_asked_for() {
    use crate::updates::Checked;
    let mut h = rig(demo_snapshot());
    click(&mut h, "Settings");
    h.state_mut().ui.can_check_updates = true;
    h.state_mut().ui.update_check = Checked::UpToDate;
    step(&mut h);
    assert!(!h.state_mut().ui.take_update_check_request());
    click(&mut h, "Check for updates");
    assert!(h.state_mut().ui.take_update_check_request());
    assert!(!h.state_mut().ui.take_update_check_request(), "asked once");
}
