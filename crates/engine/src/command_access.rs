//! The commands automation may run.
//!
//! `deckcraft-cli mcp --allow-commands <list>` keeps an agent to the commands it was given, the
//! way `--automation-read-root` / `--automation-write-root` ([`crate::file_access`]) keep it to
//! folders. While a [`CommandAllowlist`] is in force:
//!
//! - every command a client asks for (MCP `run_command`, each command of `batch`, and the commands
//!   behind the dedicated tools: `new_presentation` is `file.new`, `save_presentation` is
//!   `file.save`, `add_slide` is `slide.new`, `inspect_document` is `document.inspect`…) must be on
//!   the list;
//! - gestures (`pointer`, `key`, `type_text`, `select_tool`) are refused: they change the
//!   presentation without naming a command (a drag moves a shape, a shortcut runs whatever it is
//!   bound to), so the list could not hold them. The commands behind them (`text.insert`,
//!   `shape.insert`, `edit.select`…) stay available by name.
//!
//! Commands a command runs itself (typing `* ` turning a paragraph into a list) are part of the
//! command that was allowed and aren't checked again. Rendering a slide reads the presentation
//! without a command and stays available.
//!
//! A list names command ids (`format.bold`) and groups (`format.*`, every command whose id starts
//! with `format.`), separated by commas, spaces or new lines; `#` starts a comment. An id no
//! command has, or a group that matches no command, is an error, so a typo can't quietly leave the
//! agent with less (or, after a rename, a different command) than intended.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use crate::command_specs;

/// Longest list accepted (bytes), so a list read from a file can't exhaust memory.
pub const MAX_LIST_BYTES: usize = 256 * 1024;

/// The commands automation may run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAllowlist {
    ids: BTreeSet<String>,
    prefixes: BTreeSet<String>,
}

impl CommandAllowlist {
    /// Parse `list` against DeckCraft's commands. An unknown id, a group that matches no command,
    /// a malformed pattern or an empty list is an error: an empty list would allow nothing, which
    /// is never what someone typing the flag meant.
    pub fn parse(list: &str) -> Result<Self, String> {
        if list.len() > MAX_LIST_BYTES {
            return Err(format!("command list too long: {} bytes (at most {MAX_LIST_BYTES})", list.len()));
        }
        let mut ids = BTreeSet::new();
        let mut prefixes = BTreeSet::new();
        for line in list.lines() {
            let line = line.split_once('#').map_or(line, |(before, _)| before);
            for item in line.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()) {
                if let Some(prefix) = item.strip_suffix(".*") {
                    if prefix.is_empty() || prefix.contains('*') {
                        return Err(format!("bad command group `{item}`: use `group.*`"));
                    }
                    let dotted = format!("{prefix}.");
                    if !command_specs().iter().any(|spec| spec.id.starts_with(&dotted)) {
                        return Err(format!("`{item}` matches no command"));
                    }
                    prefixes.insert(dotted);
                } else if item.contains('*') {
                    return Err(format!("bad command pattern `{item}`: only a trailing `.*` is allowed"));
                } else if command_specs().iter().any(|spec| spec.id == item) {
                    ids.insert(item.to_string());
                } else {
                    return Err(format!("unknown command `{item}` (deckcraft-cli commands lists them)"));
                }
            }
        }
        if ids.is_empty() && prefixes.is_empty() {
            return Err("the command list is empty".into());
        }
        Ok(Self { ids, prefixes })
    }

    /// Whether `id` is on the list.
    pub fn allows(&self, id: &str) -> bool {
        self.ids.contains(id) || self.prefixes.iter().any(|p| id.starts_with(p.as_str()))
    }

    /// The listed ids and groups (`group.*`), sorted, for logs and messages.
    pub fn describe(&self) -> Vec<String> {
        self.ids.iter().cloned().chain(self.prefixes.iter().map(|p| format!("{p}*"))).collect()
    }
}

thread_local! {
    /// This thread's list; `None`: the process's ([`confine_process`]).
    static SCOPE: RefCell<Option<Arc<CommandAllowlist>>> = const { RefCell::new(None) };
}

/// The list in force in every thread outside [`confine`].
static PROCESS: OnceLock<Arc<CommandAllowlist>> = OnceLock::new();

/// Put `list` in force in the whole process, for good. `false` when a list was in force already
/// (it stays as it was).
pub fn confine_process(list: Arc<CommandAllowlist>) -> bool {
    PROCESS.set(list).is_ok()
}

/// Puts the list it replaced back when dropped (also when `f` panics).
struct Restore(Option<Arc<CommandAllowlist>>);

impl Drop for Restore {
    /// Put the thread's previous list (or none) back in force.
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPE.with(|s| *s.borrow_mut() = previous);
    }
}

/// Run `f` with `list` in force on this thread (`None`: as things stand).
pub fn confine<T>(list: Option<&Arc<CommandAllowlist>>, f: impl FnOnce() -> T) -> T {
    match list {
        Some(l) => {
            let _restore = Restore(SCOPE.with(|s| s.replace(Some(l.clone()))));
            f()
        }
        None => f(),
    }
}

/// The list in force on this thread, if any.
pub fn current() -> Option<Arc<CommandAllowlist>> {
    SCOPE.with(|s| s.borrow().clone()).or_else(|| PROCESS.get().cloned())
}

/// May automation run `id` here? Always, without a list in force.
pub fn check(id: &str) -> Result<(), String> {
    match current() {
        Some(list) if !list.allows(id) => Err(format!("command `{id}` is not allowed: it is not on this session's --allow-commands list")),
        _ => Ok(()),
    }
}

/// May automation use the gesture `what` (`pointer`, `key`, `type_text`, `select_tool`)? Not while
/// a list is in force: a gesture changes the presentation without naming a command.
pub fn check_gesture(what: &str) -> Result<(), String> {
    match current() {
        Some(_) => Err(format!(
            "`{what}` is not available with --allow-commands: gestures change the presentation without naming a command; \
use the commands behind them (text.insert, shape.insert, edit.select…) by name"
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_groups_and_comments() {
        let l = CommandAllowlist::parse("text.insert, format.*\n# saving\nfile.save  # in place\n").unwrap();
        assert!(l.allows("text.insert") && l.allows("format.bold") && l.allows("file.save"));
        assert!(!l.allows("file.open") && !l.allows("formatx.bold") && !l.allows("slide.delete"));
        // `format.*` is the `format.` group, not every id starting with "format".
        assert!(!l.allows("format"));
        assert_eq!(l.describe(), vec!["file.save", "text.insert", "format.*"]);
    }

    #[test]
    fn typos_and_empty_lists_are_errors() {
        for bad in ["", "  # nothing\n", "format.bld", "nope.*", "*", ".*", "format*", "fo*rmat.bold", "format.**"] {
            assert!(CommandAllowlist::parse(bad).is_err(), "{bad:?}");
        }
        assert!(CommandAllowlist::parse(&"x".repeat(MAX_LIST_BYTES + 1)).is_err());
    }

    #[test]
    fn a_thread_scope_wins_and_is_restored() {
        let l = Arc::new(CommandAllowlist::parse("text.insert").unwrap());
        assert!(check("file.open").is_ok(), "no list: everything");
        assert!(check_gesture("pointer").is_ok(), "no list: gestures too");
        confine(Some(&l), || {
            assert!(check("text.insert").is_ok());
            let e = check("file.open").unwrap_err();
            assert!(e.contains("file.open") && e.contains("--allow-commands"), "{e}");
            assert!(check_gesture("pointer").unwrap_err().contains("--allow-commands"));
        });
        assert!(check("file.open").is_ok(), "the scope ends with confine");
        assert!(check_gesture("key").is_ok());
    }
}
