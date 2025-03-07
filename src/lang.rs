use byte_unit::Byte;
use fluent::{bundle::FluentBundle, FluentArgs, FluentResource};
use intl_memoizer::concurrent::IntlLangMemoizer;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Mutex;
use unic_langid::LanguageIdentifier;

use crate::{
    config::SortKey,
    manifest::Store,
    prelude::{Error, OperationStatus, OperationStepDecision, StrictPath},
};

const PATH: &str = "path";
const PATH_ACTION: &str = "path-action";
const PROCESSED_GAMES: &str = "processed-games";
const PROCESSED_SIZE: &str = "processed-size";
const TOTAL_GAMES: &str = "total-games";
const TOTAL_SIZE: &str = "total-size";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    English,
}

impl Language {
    pub fn id(&self) -> String {
        match self {
            Self::English => "en-US",
        }
        .to_string()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Translator {}

static BUNDLE: Lazy<Mutex<FluentBundle<FluentResource, IntlLangMemoizer>>> = Lazy::new(|| {
    let ftl = include_str!("../lang/en-US.ftl").to_owned();
    let res = FluentResource::try_new(ftl).expect("Failed to parse Fluent file content.");

    let language_id: LanguageIdentifier = Language::English.id().parse().unwrap();
    let mut bundle = FluentBundle::new_concurrent(vec![language_id]);
    bundle.set_use_isolating(false);

    bundle
        .add_resource(res)
        .expect("Failed to add Fluent resources to the bundle.");

    Mutex::new(bundle)
});

static RE_EXTRA_SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r#"([^\r\n ]) {2,}"#).unwrap());
static RE_EXTRA_LINES: Lazy<Regex> = Lazy::new(|| Regex::new(r#"([^\r\n ])[\r\n]([^\r\n ])"#).unwrap());
static RE_EXTRA_PARAGRAPHS: Lazy<Regex> = Lazy::new(|| Regex::new(r#"([^\r\n ])[\r\n]{2,}([^\r\n ])"#).unwrap());

fn translate(id: &str) -> String {
    translate_args(id, &FluentArgs::new())
}

fn translate_args(id: &str, args: &FluentArgs) -> String {
    let bundle = match BUNDLE.lock() {
        Ok(x) => x,
        Err(_) => return "fluent-cannot-lock".to_string(),
    };

    let parts: Vec<&str> = id.splitn(2, '.').collect();
    let (name, attr) = if parts.len() < 2 {
        (id, None)
    } else {
        (parts[0], Some(parts[1]))
    };

    let message = match bundle.get_message(name) {
        Some(x) => x,
        None => return format!("fluent-no-message={}", name),
    };

    let pattern = match attr {
        None => match message.value() {
            Some(x) => x,
            None => return format!("fluent-no-message-value={}", id),
        },
        Some(attr) => match message.get_attribute(attr) {
            Some(x) => x.value(),
            None => return format!("fluent-no-attr={}", id),
        },
    };
    let mut errors = vec![];
    let value = bundle.format_pattern(pattern, Some(args), &mut errors);

    RE_EXTRA_PARAGRAPHS
        .replace_all(
            &RE_EXTRA_LINES.replace_all(&RE_EXTRA_SPACES.replace_all(&value, "${1} "), "${1} ${2}"),
            "${1}\n\n${2}",
        )
        .to_string()
}

impl Translator {
    pub fn window_title(&self) -> String {
        let name = translate("ludusavi");
        let version = option_env!("LUDUSAVI_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
        match option_env!("LUDUSAVI_VARIANT") {
            Some(variant) => format!("{} v{} ({})", name, version, variant),
            None => format!("{} v{}", name, version),
        }
    }

    pub fn handle_error(&self, error: &Error) -> String {
        match error {
            Error::ConfigInvalid { why } => self.config_is_invalid(why),
            Error::ManifestInvalid { why } => self.manifest_is_invalid(why),
            Error::ManifestCannotBeUpdated => self.manifest_cannot_be_updated(),
            Error::CliBackupTargetExists { path } => self.cli_backup_target_exists(path),
            Error::CliUnrecognizedGames { games } => self.cli_unrecognized_games(games),
            Error::CliUnableToRequestConfirmation => self.cli_unable_to_request_confirmation(),
            Error::SomeEntriesFailed => self.some_entries_failed(),
            Error::CannotPrepareBackupTarget { path } => self.cannot_prepare_backup_target(path),
            Error::RestorationSourceInvalid { path } => self.restoration_source_is_invalid(path),
            Error::RegistryIssue => self.registry_issue(),
            Error::UnableToBrowseFileSystem => self.unable_to_browse_file_system(),
            Error::UnableToOpenDir(path) => self.unable_to_open_dir(path),
            Error::UnableToOpenUrl(url) => self.unable_to_open_url(url),
        }
    }

    pub fn cli_backup_target_exists(&self, path: &StrictPath) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, path.render());
        translate_args("cli-backup-target-already-exists", &args)
    }

    pub fn cli_unrecognized_games(&self, games: &[String]) -> String {
        let prefix = translate("cli-unrecognized-games");
        let lines: Vec<_> = games.iter().map(|x| format!("  - {}", x)).collect();
        format!("{}\n{}", prefix, lines.join("\n"))
    }

    pub fn cli_confirm_restoration(&self, path: &StrictPath) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, path.render());
        translate_args("cli-confirm-restoration", &args)
    }

    pub fn cli_unable_to_request_confirmation(&self) -> String {
        #[cfg(target_os = "windows")]
        let extra_note = translate("cli-unable-to-request-confirmation.winpty-workaround");

        #[cfg(not(target_os = "windows"))]
        let extra_note = "";

        format!("{} {}", translate("cli-unable-to-request-confirmation"), extra_note)
    }

    pub fn some_entries_failed(&self) -> String {
        translate("some-entries-failed")
    }

    fn label(&self, text: &str) -> String {
        format!("[{}]", text)
    }

    pub fn label_failed(&self) -> String {
        self.label(&self.badge_failed())
    }

    pub fn label_duplicates(&self) -> String {
        self.label(&self.badge_duplicates())
    }

    pub fn label_duplicated(&self) -> String {
        self.label(&self.badge_duplicated())
    }

    pub fn label_ignored(&self) -> String {
        self.label(&self.badge_ignored())
    }

    pub fn badge_failed(&self) -> String {
        translate("badge-failed")
    }

    pub fn badge_duplicates(&self) -> String {
        translate("badge-duplicates")
    }

    pub fn badge_duplicated(&self) -> String {
        translate("badge-duplicated")
    }

    pub fn badge_ignored(&self) -> String {
        translate("badge-ignored")
    }

    pub fn badge_redirected_from(&self, original: &StrictPath) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, original.render());
        translate_args("badge-redirected-from", &args)
    }

    pub fn cli_game_header(
        &self,
        name: &str,
        bytes: u64,
        decision: &OperationStepDecision,
        duplicated: bool,
    ) -> String {
        let mut labels = vec![];
        if *decision == OperationStepDecision::Ignored {
            labels.push(self.label_ignored());
        }
        if duplicated {
            labels.push(self.label_duplicates());
        }

        if labels.is_empty() {
            format!("{} [{}]:", name, self.adjusted_size(bytes))
        } else {
            format!("{} [{}] {}:", name, self.adjusted_size(bytes), labels.join(" "))
        }
    }

    pub fn cli_game_line_item(&self, item: &str, successful: bool, ignored: bool, duplicated: bool) -> String {
        let mut parts = vec![];
        if !successful {
            parts.push(self.label_failed());
        }
        if ignored {
            parts.push(self.label_ignored());
        }
        if duplicated {
            parts.push(self.label_duplicated());
        }
        parts.push(item.to_string());

        format!("  - {}", parts.join(" "))
    }

    pub fn cli_game_line_item_redirected(&self, item: &str) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, item);
        translate_args("cli-game-line-redirected-from", &args)
    }

    pub fn cli_summary(&self, status: &OperationStatus, location: &StrictPath) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, location.render());
        args.set(TOTAL_GAMES, status.total_games);
        args.set(PROCESSED_GAMES, status.processed_games);
        args.set(TOTAL_SIZE, self.adjusted_size(status.total_bytes));
        args.set(PROCESSED_SIZE, self.adjusted_size(status.processed_bytes));

        if status.processed_all() {
            translate_args("cli-summary.succeeded", &args)
        } else {
            translate_args("cli-summary.failed", &args)
        }
    }

    pub fn backup_button(&self) -> String {
        translate("button-backup")
    }

    pub fn preview_button(&self) -> String {
        translate("button-preview")
    }

    pub fn restore_button(&self) -> String {
        translate("button-restore")
    }

    pub fn nav_backup_button(&self) -> String {
        translate("button-nav-backup")
    }

    pub fn nav_restore_button(&self) -> String {
        translate("button-nav-restore")
    }

    pub fn nav_custom_games_button(&self) -> String {
        translate("button-nav-custom-games")
    }

    pub fn nav_other_button(&self) -> String {
        translate("button-nav-other")
    }

    pub fn add_root_button(&self) -> String {
        translate("button-add-root")
    }

    pub fn find_roots_button(&self) -> String {
        translate("button-find-roots")
    }

    pub fn no_missing_roots(&self) -> String {
        translate("no-missing-roots")
    }

    pub fn confirm_add_missing_roots(&self, roots: &[crate::config::RootsConfig]) -> String {
        use std::fmt::Write;
        let mut msg = translate("confirm-add-missing-roots") + "\n";

        for root in roots {
            let _ = &write!(msg, "\n[{}] {}", self.store(&root.store), root.path.render());
        }

        msg
    }

    pub fn add_redirect_button(&self) -> String {
        translate("button-add-redirect")
    }

    pub fn add_game_button(&self) -> String {
        translate("button-add-game")
    }

    pub fn continue_button(&self) -> String {
        translate("button-continue")
    }

    pub fn cancel_button(&self) -> String {
        translate("button-cancel")
    }

    pub fn cancelling_button(&self) -> String {
        translate("button-cancelling")
    }

    pub fn okay_button(&self) -> String {
        translate("button-okay")
    }

    pub fn select_all_button(&self) -> String {
        translate("button-select-all")
    }

    pub fn deselect_all_button(&self) -> String {
        translate("button-deselect-all")
    }

    pub fn enable_all_button(&self) -> String {
        translate("button-enable-all")
    }

    pub fn disable_all_button(&self) -> String {
        translate("button-disable-all")
    }

    pub fn no_roots_are_configured(&self) -> String {
        translate("no-roots-are-configured")
    }

    pub fn config_is_invalid(&self, why: &str) -> String {
        format!("{}\n{}", translate("config-is-invalid"), why)
    }

    pub fn manifest_is_invalid(&self, why: &str) -> String {
        format!("{}\n{}", translate("manifest-is-invalid"), why)
    }

    pub fn manifest_cannot_be_updated(&self) -> String {
        translate("manifest-cannot-be-updated")
    }

    pub fn cannot_prepare_backup_target(&self, target: &StrictPath) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, target.render());
        translate_args("cannot-prepare-backup-target", &args)
    }

    pub fn restoration_source_is_invalid(&self, source: &StrictPath) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, source.render());
        translate_args("restoration-source-is-invalid", &args)
    }

    pub fn registry_issue(&self) -> String {
        translate("registry-issue")
    }

    pub fn unable_to_browse_file_system(&self) -> String {
        translate("unable-to-browse-file-system")
    }

    pub fn unable_to_open_dir(&self, path: &StrictPath) -> String {
        format!("{}\n\n{}", translate("unable-to-open-directory"), path.render())
    }

    pub fn unable_to_open_url(&self, url: &str) -> String {
        format!("{}\n\n{}", translate("unable-to-open-url"), url)
    }

    pub fn adjusted_size(&self, bytes: u64) -> String {
        let byte = Byte::from_bytes(bytes.into());
        let adjusted_byte = byte.get_appropriate_unit(true);
        adjusted_byte.to_string()
    }

    pub fn processed_games(&self, status: &OperationStatus) -> String {
        let mut args = FluentArgs::new();
        args.set(TOTAL_GAMES, status.total_games);
        args.set(PROCESSED_GAMES, status.processed_games);

        if status.processed_all_games() {
            translate_args("processed-games", &args)
        } else {
            translate_args("processed-games-subset", &args)
        }
    }

    pub fn processed_bytes(&self, status: &OperationStatus) -> String {
        if status.processed_all_bytes() {
            self.adjusted_size(status.total_bytes)
        } else {
            let mut args = FluentArgs::new();
            args.set(TOTAL_SIZE, self.adjusted_size(status.total_bytes));
            args.set(PROCESSED_SIZE, self.adjusted_size(status.processed_bytes));
            translate_args("processed-size-subset", &args)
        }
    }

    pub fn processed_subset(&self, total: usize, processed: usize) -> String {
        let mut args = FluentArgs::new();
        args.set(TOTAL_SIZE, total as u64);
        args.set(PROCESSED_SIZE, processed as u64);
        translate_args("processed-size-subset", &args)
    }

    pub fn backup_target_label(&self) -> String {
        translate("field-backup-target")
    }

    pub fn backup_merge_label(&self) -> String {
        translate("toggle-backup-merge")
    }

    pub fn restore_source_label(&self) -> String {
        translate("field-restore-source")
    }

    pub fn custom_files_label(&self) -> String {
        translate("field-custom-files")
    }

    pub fn custom_registry_label(&self) -> String {
        translate("field-custom-registry")
    }

    pub fn search_label(&self) -> String {
        translate("field-search")
    }

    pub fn sort_label(&self) -> String {
        translate("field-sort")
    }

    pub fn store(&self, store: &Store) -> String {
        translate(match store {
            Store::Epic => "store-epic",
            Store::Gog => "store-gog",
            Store::GogGalaxy => "store-gog-galaxy",
            Store::Microsoft => "store-microsoft",
            Store::Origin => "store-origin",
            Store::Prime => "store-prime",
            Store::Steam => "store-steam",
            Store::Uplay => "store-uplay",
            Store::OtherHome => "store-other-home",
            Store::OtherWine => "store-other-wine",
            Store::Other => "store-other",
        })
    }

    pub fn sort_key(&self, key: &SortKey) -> String {
        translate(match key {
            SortKey::Name => "sort-name",
            SortKey::Size => "sort-size",
        })
    }

    pub fn sort_reversed(&self) -> String {
        translate("sort-reversed")
    }

    pub fn redirect_source_placeholder(&self) -> String {
        translate("field-redirect-source.placeholder")
    }

    pub fn redirect_target_placeholder(&self) -> String {
        translate("field-redirect-target.placeholder")
    }

    pub fn custom_game_name_placeholder(&self) -> String {
        translate("field-custom-game-name.placeholder")
    }

    pub fn search_game_name_placeholder(&self) -> String {
        translate("field-search-game-name.placeholder")
    }

    pub fn explanation_for_exclude_other_os_data(&self) -> String {
        translate("explanation-for-exclude-other-os-data")
    }

    pub fn explanation_for_exclude_store_screenshots(&self) -> String {
        translate("explanation-for-exclude-store-screenshots")
    }

    pub fn ignored_items_label(&self) -> String {
        translate("field-backup-excluded-items")
    }

    pub fn full_retention(&self) -> String {
        translate("field-retention-full")
    }

    pub fn differential_retention(&self) -> String {
        translate("field-retention-differential")
    }

    pub fn modal_confirm_backup(&self, target: &StrictPath, target_exists: bool, merge: bool) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, target.render());
        args.set(
            PATH_ACTION,
            match (target_exists, merge) {
                (false, _) => "create",
                (true, false) => "recreate",
                (true, true) => "merge",
            },
        );
        translate_args("confirm-backup", &args)
    }

    pub fn modal_confirm_restore(&self, source: &StrictPath) -> String {
        let mut args = FluentArgs::new();
        args.set(PATH, source.render());
        translate_args("confirm-restore", &args)
    }
}
