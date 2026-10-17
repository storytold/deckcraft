use serde_json::json;

use crate::{Manifest, parse_reply};

#[test]
fn manifests_are_validated() {
    // A shared manifest's spreadsheet functions are checked, and ignored here.
    let with = |functions: serde_json::Value| json!({"id": "t.plugin", "name": "T", "run": {"command": "true"}, "functions": functions}).to_string();
    let m = Manifest::parse(&with(json!([{"name": "t.one", "maxArgs": 2}]))).unwrap();
    assert_eq!(m.functions[0].name, "T.ONE");
    assert!(Manifest::parse(&with(json!([{"name": "1BAD"}]))).is_err());
    assert!(Manifest::parse(r#"{"id": "bad id", "name": "x", "run": {"command": "x"}}"#).is_err());
    assert!(Manifest::parse(r#"{"id": "x", "name": "x", "run": {"command": ""}}"#).is_err());
    assert!(Manifest::parse("not json").is_err());
}

#[test]
fn one_manifest_serves_several_apps() {
    // GridCraft's row menu means nothing here; `uiFor.deckcraft` replaces `ui`.
    let all = json!({
        "id": "t.all", "name": "All", "run": {"command": "true"}, "apps": ["gridcraft", "wordcraft", "DeckCraft"],
        "functions": [{"name": "T.TWICE", "minArgs": 1, "maxArgs": 1}],
        "commands": [{"id": "tidy", "label": "Tidy"}],
        "ui": {"menus": [{"menu": "row", "command": "tidy"}]},
        "uiFor": {"deckcraft": {"menus": [{"menu": "slide", "command": "tidy"}]}, "wordcraft": {"menus": [{"menu": "picture", "command": "tidy"}]}}
    });
    let m = Manifest::parse(&all.to_string()).unwrap();
    assert_eq!(m.ui.menus.len(), 1);
    assert_eq!(m.ui.menus[0].menu, crate::MenuKind::Slide);
    assert!(m.for_this_app());
    let only_excel = json!({"id": "t.xl", "name": "Excel", "run": {"command": "true"}, "apps": ["gridcraft"]});
    assert!(Manifest::parse(&only_excel.to_string()).unwrap_err().contains("not DeckCraft"));
    // Without `uiFor.deckcraft`, `ui` is this app's: another app's menu names are an error.
    let plain = json!({"id": "t.p", "name": "P", "run": {"command": "true"}, "commands": [{"id": "go", "label": "Go"}], "ui": {"menus": [{"menu": "row", "command": "go"}]}});
    assert!(Manifest::parse(&plain.to_string()).is_err());
}

#[test]
fn ui_declarations_must_name_the_plugins_commands() {
    let with_ui = |ui: serde_json::Value| {
        json!({"id": "t.ui", "name": "T", "run": {"command": "true"},
            "commands": [{"id": "go", "label": "Go"}], "ui": ui})
        .to_string()
    };
    let m = Manifest::parse(&with_ui(json!({
        "ribbon": [{"tab": "Design", "group": "Mine", "buttons": [{"command": "go", "size": "small", "params": {"n": 1}}]}],
        "menus": [{"command": "go"}, {"menu": "text", "command": "go", "label": "Go here"}]
    })))
    .unwrap();
    assert_eq!(m.ui.ribbon[0].buttons[0].size, crate::ButtonSize::Small);
    assert_eq!(m.ui.menus[0].menu, crate::MenuKind::Shape);
    assert_eq!(m.ui.menus[1].menu, crate::MenuKind::Text);
    assert_eq!(m.command("go").map(|c| c.label.as_str()), Some("Go"));
    for bad in [
        json!({"ribbon": [{"tab": "Design", "group": "G", "buttons": [{"command": "nope"}]}]}),
        json!({"ribbon": [{"tab": "Design", "group": "G", "buttons": []}]}),
        json!({"ribbon": [{"tab": " ", "group": "G", "buttons": [{"command": "go"}]}]}),
        json!({"ribbon": [{"tab": "Design", "group": "G", "buttons": [{"command": "go", "size": "huge"}]}]}),
        json!({"menus": [{"command": "nope"}]}),
        json!({"menus": [{"menu": "statusBar", "command": "go"}]}),
        json!({"menus": [{"command": "go", "label": "x".repeat(65)}]}),
    ] {
        assert!(Manifest::parse(&with_ui(bad.clone())).is_err(), "{bad}");
    }
}

#[test]
fn homepages_must_be_web_links() {
    let with = |url: &str| json!({"id": "t.home", "name": "T", "run": {"command": "true"}, "homepage": url}).to_string();
    assert_eq!(Manifest::parse(&with("https://example.org/doc")).unwrap().homepage, "https://example.org/doc");
    assert!(Manifest::parse(&with("")).is_ok());
    for bad in ["javascript:alert(1)", "file:///etc/passwd", "example.org", "https://a b"] {
        assert!(Manifest::parse(&with(bad)).is_err(), "{bad}");
    }
}

#[test]
fn turned_off_plugins_lose_their_commands() {
    let m = Manifest::parse(
        &json!({"id": "t.toggle", "name": "T", "run": {"command": "./never-started"}, "commands": [{"id": "go", "label": "Go"}]}).to_string(),
    )
    .unwrap();
    crate::install(m, std::env::temp_dir()).unwrap();
    assert_eq!(crate::set_enabled("t.toggle", false), Ok(true));
    assert_eq!(crate::set_enabled("t.toggle", false), Ok(false));
    assert_eq!(crate::get("t.toggle").map(|p| p.status()), Some("off".into()));
    assert!(crate::run_command("t.toggle", "go", json!({}), json!({})).unwrap_err().contains("turned off"));
    assert_eq!(crate::set_enabled("t.toggle", true), Ok(true));
    assert!(crate::unload("t.toggle"));
    assert!(crate::set_enabled("t.toggle", true).is_err());
}

#[test]
fn command_replies_are_checked() {
    let r = parse_reply(json!({"commands": [{"command": "slide.new", "params": {"title": "Hi"}}], "message": "hi"})).unwrap();
    assert_eq!(r.commands.len(), 1);
    assert_eq!(r.message.as_deref(), Some("hi"));
    assert!(parse_reply(json!({"commands": {}})).is_err());
    assert!(parse_reply(json!({"commands": [{"params": {}}]})).is_err());
    assert!(parse_reply(json!(null)).unwrap().commands.is_empty());
}

#[test]
fn initialize_announces_protocols_1_2_and_2() {
    let p = crate::initialize_params("t.init", std::path::Path::new("/tmp"));
    assert_eq!(p["protocolVersion"], 1, "protocolVersion stays 1");
    assert_eq!(p["host"]["protocol"], "2.0", "manifests 1.3: host.protocol");
    assert_eq!(p["host"]["features"], json!(["dialogs", "open", "events", "document", "push", "collab"]));
    assert_eq!(p["host"]["collab"], json!({"formats": ["yjs-v1"], "app": "deckcraft", "schema": 1, "encryption": ["aes-256-gcm"]}));
    assert_eq!(p["host"]["language"], "en");
    crate::set_host_language("not a tag!");
    assert_eq!(crate::host_language(), "en", "a bad tag is ignored");
}

#[test]
fn transports_declare_themselves_and_answer_with_rooms() {
    let m = Manifest::parse(
        &json!({"id": "t.tr", "name": "T", "run": {"command": "true"}, "collab": {"transport": "mqtt", "label": "Live (MQTT)"}}).to_string(),
    )
    .unwrap();
    assert_eq!(m.collab.as_ref().map(|c| c.transport.as_str()), Some("mqtt"));
    for bad in [json!({"transport": ""}), json!({"transport": "x".repeat(65)}), json!({"transport": "m", "label": "l".repeat(65)})] {
        assert!(Manifest::parse(&json!({"id": "t.tr", "name": "T", "run": {"command": "true"}, "collab": bad}).to_string()).is_err(), "{bad}");
    }
    let r = parse_reply(json!({"collab": {"join": {"room": "mqtt:x", "mode": "share"}}, "message": "hi"})).unwrap();
    assert_eq!(r.collab_join.unwrap()["room"], "mqtt:x");
    assert!(parse_reply(json!({"collab": {"join": "nope"}})).is_err());
    assert!(parse_reply(json!({"collab": {"join": null}})).unwrap().collab_join.is_none());
}

#[test]
fn manifests_declare_the_events_they_hear() {
    let with = |events: serde_json::Value| json!({"id": "t.ev", "name": "T", "run": {"command": "true"}, "events": events}).to_string();
    let m = Manifest::parse(&with(json!(["documentSaved", "somethingLater"]))).unwrap();
    assert_eq!(m.events, ["documentSaved", "somethingLater"]);
    assert!(Manifest::parse(&with(json!("documentSaved"))).is_err());
    assert!(Manifest::parse(&with(json!([""]))).is_err());
    assert!(Manifest::parse(&with(json!(vec!["documentSaved"; 65]))).is_err());
    let none = Manifest::parse(&json!({"id": "t.ev", "name": "T", "run": {"command": "true"}}).to_string()).unwrap();
    assert!(none.events.is_empty());
    crate::install(m, std::env::temp_dir()).unwrap();
    assert_eq!(crate::listeners("documentSaved"), ["t.ev"]);
    assert!(crate::listeners("documentOpened").is_empty(), "only what it declared");
    crate::set_enabled("t.ev", false).unwrap();
    assert!(crate::listeners("documentSaved").is_empty(), "a plugin turned off hears nothing");
    let mut refuse = |_: &str, _: &serde_json::Value| Err(crate::HostError::unknown());
    assert!(crate::send_event("t.ev", "documentSaved", json!({}), &mut refuse).is_err());
    assert!(crate::unload("t.ev"));
}

#[test]
fn replies_may_open_a_file_or_show_a_dialog() {
    let r = parse_reply(json!({"open": {"path": "/tmp/a.pptx", "label": "x".repeat(300), "readOnly": true}})).unwrap();
    let open = r.open.unwrap();
    assert_eq!((open.path.as_str(), open.label.as_ref().map(|l| l.chars().count()), open.read_only), ("/tmp/a.pptx", Some(200), true));
    for bad in [json!({"open": "a.pptx"}), json!({"open": {}}), json!({"open": {"path": ""}}), json!({"open": {"path": "/a", "readOnly": "yes"}})] {
        assert!(parse_reply(bad.clone()).is_err(), "{bad}");
    }

    let list = json!({"dialog": {
        "title": "Open from Nextcloud", "kind": "list", "prompt": "Documents",
        "items": [{"id": "/Documents/Reports", "label": "Reports", "detail": "Folder", "icon": "folder"}, {"id": 7}],
        "command": "browse", "params": {"account": "me"}, "okLabel": "Open",
        "buttons": [{"label": "Up", "command": "browse", "params": {"path": "/"}}]
    }});
    let d = parse_reply(list).unwrap().dialog.unwrap();
    assert_eq!(d.kind, crate::dialog::DialogKind::List);
    assert_eq!(d.items.len(), 2);
    assert_eq!((d.items[1].id.as_str(), d.items[1].label.as_str()), ("7", "7"), "a label defaults to the id");
    assert_eq!(d.ok_label, "Open");
    assert_eq!(d.commands().collect::<Vec<_>>(), ["browse", "browse"]);
    // Selections go back as `id` (or `ids` with `multi`), forms as `fields`.
    let p = crate::dialog::action_params(&d.params, &d, &["/Documents/Reports".into()], None);
    assert_eq!(p, json!({"account": "me", "id": "/Documents/Reports"}));
    let multi = crate::dialog::Dialog { multi: true, ..d.clone() };
    assert_eq!(crate::dialog::action_params(&d.buttons[0].params, &multi, &["a".into(), "b".into()], None), json!({"path": "/", "ids": ["a", "b"]}));
    assert_eq!(crate::dialog::action_params(&d.params, &d, &[], None), json!({"account": "me"}), "no selection, no id");

    // Over a limit is cut, never refused.
    let items: Vec<serde_json::Value> = (0..10_050).map(|i| json!({"id": format!("{i}{}", "x".repeat(5000)), "label": "l".repeat(400)})).collect();
    let big = parse_reply(json!({"dialog": {"kind": "list", "title": "t".repeat(500), "items": items, "command": "go",
        "buttons": (0..9).map(|_| json!({"label": "B", "command": "go"})).collect::<Vec<_>>()}}))
    .unwrap()
    .dialog
    .unwrap();
    assert_eq!((big.items.len(), big.buttons.len(), big.title.chars().count()), (10_000, 4, 200));
    assert_eq!((big.items[0].id.chars().count(), big.items[0].label.chars().count()), (4096, 256));

    let form = json!({"dialog": {"kind": "form", "title": "Sign in", "command": "login", "fields": [
        {"id": "user", "label": "User", "value": "me"},
        {"id": "pw", "label": "Password", "kind": "secret"},
        {"id": "n", "kind": "number", "value": 3},
        {"id": "keep", "kind": "checkbox", "value": true},
        {"id": "srv", "kind": "choice", "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}], "value": "b"},
        {"id": "srv2", "kind": "choice", "options": [{"id": "a", "label": "A"}], "value": "zzz"}
    ]}});
    let f = parse_reply(form).unwrap().dialog.unwrap();
    assert_eq!(f.fields.len(), 6);
    assert_eq!(f.fields[2].value, json!("3"));
    assert_eq!(f.fields[3].value, json!(true));
    assert_eq!(f.fields[4].value, json!("b"));
    assert_eq!(f.fields[5].value, json!("a"), "an unknown choice starts at the first option");
    assert_eq!(f.ok_label, "OK");
    let mut fields = serde_json::Map::new();
    fields.insert("user".into(), json!("me"));
    assert_eq!(crate::dialog::action_params(&f.params, &f, &[], Some(fields)), json!({"fields": {"user": "me"}}));
    assert_eq!(crate::dialog::number_value(" 2.5 "), Some(json!(2.5)));
    assert_eq!(crate::dialog::number_value("4"), Some(json!(4)));
    assert_eq!(crate::dialog::number_value(""), Some(json!(null)));
    assert_eq!(crate::dialog::number_value("NaN"), None);
    assert_eq!(crate::dialog::number_value("four"), None);

    let msg = parse_reply(json!({"dialog": {"kind": "message", "prompt": "p".repeat(5000)}})).unwrap().dialog.unwrap();
    assert_eq!((msg.prompt.chars().count(), msg.command.is_none()), (4000, true), "a message needs no command");

    // Malformed: an error for the command, never a crash.
    for bad in [
        json!({"dialog": "hi"}),
        json!({"dialog": {"kind": "web"}}),
        json!({"dialog": {"title": "no kind"}}),
        json!({"dialog": {"kind": "list", "command": "go"}}),
        json!({"dialog": {"kind": "list", "items": []}}),
        json!({"dialog": {"kind": "list", "items": [{"label": "no id"}], "command": "go"}}),
        json!({"dialog": {"kind": "list", "items": [], "command": "../x"}}),
        json!({"dialog": {"kind": "list", "items": [], "command": "go", "params": [1]}}),
        json!({"dialog": {"kind": "form", "command": "go", "fields": [{"id": "x", "kind": "slider"}]}}),
        json!({"dialog": {"kind": "form", "command": "go", "fields": [{"id": "x", "kind": "choice"}]}}),
        json!({"dialog": {"kind": "form", "command": "go", "fields": [{"id": "x", "kind": "checkbox", "value": "yes"}]}}),
        json!({"dialog": {"kind": "form", "command": "go", "fields": [{"label": "no id"}]}}),
        json!({"dialog": {"kind": "message", "buttons": [{"label": "B"}]}}),
        json!({"dialog": {"kind": "message", "buttons": {}}}),
        json!({"dialog": {"kind": "message", "title": 5}}),
    ] {
        assert!(parse_reply(bad.clone()).is_err(), "{bad}");
    }
}

mod install {
    use std::io::Write;

    use crate::install::{Repo, Version, parse_github_url, pick_asset, unpack};

    fn repo(owner: &str, name: &str, version: Version) -> Repo {
        Repo { owner: owner.into(), name: name.into(), version }
    }

    #[test]
    fn github_addresses_are_read() {
        for url in ["https://github.com/acme/fin-tools", "github.com/acme/fin-tools/", "http://www.github.com/acme/fin-tools.git", "acme/fin-tools"] {
            assert_eq!(parse_github_url(url), Ok(repo("acme", "fin-tools", Version::Latest)), "{url}");
        }
        assert_eq!(parse_github_url("https://github.com/acme/fin/releases/tag/v1.2.0"), Ok(repo("acme", "fin", Version::Release("v1.2.0".into()))));
        assert_eq!(parse_github_url("https://github.com/acme/fin/tree/feat/x"), Ok(repo("acme", "fin", Version::Tree("feat/x".into()))));
        assert_eq!(parse_github_url("https://github.com/acme/fin/releases/latest"), Ok(repo("acme", "fin", Version::Latest)));
        for bad in [
            "",
            "https://gitlab.com/acme/fin",
            "https://github.com/acme",
            "https://github.com/../fin",
            "https://github.com/acme/fin/blob/main/x",
            "https://github.com/acme/fin/tree/../../x",
            "ftp://github.com/acme/fin",
            "a/b/c",
        ] {
            assert!(parse_github_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_release_zip_for_this_platform_is_picked() {
        let names: Vec<String> = [
            "fin-1.0-linux-x86_64.zip",
            "fin-1.0-macos-x86_64.zip",
            "fin-1.0-macos-aarch64.zip",
            "fin-1.0-windows-x64.zip",
            "fin-1.0-macos-aarch64.tar.gz",
            "checksums.txt",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(pick_asset(&names, "macos", "aarch64"), Some(2));
        assert_eq!(pick_asset(&names, "macos", "x86_64"), Some(1));
        assert_eq!(pick_asset(&names, "windows", "x86_64"), Some(3));
        assert_eq!(pick_asset(&names, "linux", "aarch64"), None, "no build for it: install the source");
        let universal: Vec<String> = ["fin-darwin-universal.zip", "fin-linux-amd64.zip"].map(String::from).to_vec();
        assert_eq!(pick_asset(&universal, "macos", "aarch64"), Some(0));
        let any: Vec<String> = ["fin-plugin.zip", "fin-linux-arm64.zip"].map(String::from).to_vec();
        assert_eq!(pick_asset(&any, "macos", "aarch64"), Some(0), "a zip naming no platform suits every one");
        assert_eq!(pick_asset(&any, "linux", "aarch64"), Some(1), "this platform's build beats the generic zip");
        assert_eq!(pick_asset(&["darwin-tools-windows.zip".to_string()], "macos", "x86_64"), Some(0));
        assert_eq!(pick_asset(&[], "macos", "aarch64"), None);
    }

    fn zip(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut out);
        for (name, data, mode) in files {
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        out.into_inner()
    }

    fn manifest(id: &str) -> Vec<u8> {
        serde_json::json!({"id": id, "name": "Fin", "run": {"command": "./run.sh"}}).to_string().into_bytes()
    }

    #[test]
    fn a_source_zip_unpacks_into_a_folder_named_after_the_plugin() {
        let root = std::env::temp_dir().join(format!("deckcraft-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let m = manifest("acme.fin");
        let bytes = zip(&[
            ("acme-fin-1a2b3c/README.md", b"readme", 0o644),
            ("acme-fin-1a2b3c/plugin.json", &m, 0o644),
            ("acme-fin-1a2b3c/run.sh", b"#!/bin/sh\n", 0o755),
            ("acme-fin-1a2b3c/lib/util.py", b"x = 1\n", 0o644),
            ("acme-fin-1a2b3c/examples/other/plugin.json", b"{}", 0o644),
            ("elsewhere/secret.txt", b"not part of the plugin", 0o644),
        ]);
        let dir = unpack(&bytes, &root).unwrap();
        assert_eq!(dir, root.join("acme.fin"));
        assert!(dir.join("plugin.json").is_file() && dir.join("lib/util.py").is_file() && dir.join("examples/other/plugin.json").is_file());
        assert!(!dir.join("elsewhere").exists() && !root.join("elsewhere").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("run.sh")).unwrap().permissions().mode() & 0o777, 0o755);
            assert_eq!(std::fs::metadata(dir.join("README.md")).unwrap().permissions().mode() & 0o777, 0o644);
        }
        crate::load(&dir).unwrap();
        assert!(crate::unload("acme.fin"));

        // Installing again replaces the earlier copy, leftovers included.
        std::fs::write(dir.join("stale.txt"), "old").unwrap();
        let again = unpack(&zip(&[("plugin.json", &m, 0o644), ("run.sh", b"new", 0o755)]), &root).unwrap();
        assert_eq!(again, dir);
        assert!(!dir.join("stale.txt").exists() && !dir.join("lib").exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "no staging folders left behind");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unsafe_or_foreign_zips_are_refused() {
        let root = std::env::temp_dir().join(format!("deckcraft-unpack-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(unpack(b"not a zip", &root).unwrap_err().contains("not a zip"));
        assert!(unpack(&zip(&[("README.md", b"hi", 0o644)]), &root).unwrap_err().contains("plugin.json"));
        assert!(unpack(&zip(&[("plugin.json", &manifest(".."), 0o644)]), &root).is_err(), "an id can't climb out of the folder");
        assert!(unpack(&zip(&[("plugin.json", b"{", 0o644)]), &root).is_err());
        // Paths escaping the plugin folder are skipped, not written.
        let m = manifest("acme.safe");
        let dir = unpack(&zip(&[("plugin.json", &m, 0o644), ("../escaped.txt", b"x", 0o644), ("/abs.txt", b"x", 0o644)]), &root).unwrap();
        assert!(!root.join("escaped.txt").exists() && !std::env::temp_dir().join("escaped.txt").exists());
        assert!(dir.join("plugin.json").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

mod office {
    use crate::office::{Action, OfficeCommand, Permission, Step, parse_manifest};
    use crate::{MenuKind, run_command};

    /// An add-in manifest written for these tests from the public schema: a default task pane, a
    /// button on the Home tab, a menu with a function command on a custom tab, and a text menu item.
    pub(super) const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OfficeApp xmlns="http://schemas.microsoft.com/office/appforoffice/1.1" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
  xmlns:bt="http://schemas.microsoft.com/office/officeappbasictypes/1.0" xmlns:ov="http://schemas.microsoft.com/office/taskpaneappversionoverrides" xsi:type="TaskPaneApp">
  <Id>0d5c7a1e-1111-4bcd-9eaf-0123456789ab</Id>
  <Version>1.0.0.0</Version>
  <ProviderName>Acme &amp; Sons</ProviderName>
  <DefaultLocale>en-US</DefaultLocale>
  <DisplayName DefaultValue="Acme Prices"/>
  <Description DefaultValue="Live prices on your slides."/>
  <IconUrl DefaultValue="https://acme.example/icon-32.png"/>
  <SupportUrl DefaultValue="https://acme.example/help"/>
  <AppDomains><AppDomain>https://login.acme.example</AppDomain></AppDomains>
  <Hosts><Host Name="Presentation"/></Hosts>
  <Requirements><Sets DefaultMinVersion="1.1"><Set Name="PowerPointApi" MinVersion="1.3"/></Sets></Requirements>
  <DefaultSettings><SourceLocation DefaultValue="https://acme.example/taskpane.html"/></DefaultSettings>
  <Permissions>ReadWriteDocument</Permissions>
  <VersionOverrides xmlns="http://schemas.microsoft.com/office/taskpaneappversionoverrides" xsi:type="VersionOverridesV1_0">
    <Hosts>
      <Host xsi:type="Presentation">
        <DesktopFormFactor>
          <FunctionFile resid="Commands.Url"/>
          <ExtensionPoint xsi:type="PrimaryCommandSurface">
            <OfficeTab id="TabHome">
              <Group id="Acme.Group"><Label resid="Group.Label"/>
                <Control xsi:type="Button" id="Acme.Taskpane">
                  <Label resid="Button.Label"/>
                  <Supertip><Title resid="Button.Label"/><Description resid="Button.Tip"/></Supertip>
                  <Action xsi:type="ShowTaskpane"><TaskpaneId>Main</TaskpaneId><SourceLocation resid="Taskpane.Url"/></Action>
                </Control>
              </Group>
            </OfficeTab>
            <CustomTab id="Acme.Tab"><Label resid="Tab.Label"/>
              <Group id="Acme.Tools"><Label resid="Tools.Label"/>
                <Control xsi:type="Menu" id="Acme.Menu"><Label resid="Menu.Label"/>
                  <Items>
                    <Item id="Acme.Refresh"><Label resid="Refresh.Label"/><Action xsi:type="ExecuteFunction"><FunctionName>refreshAll</FunctionName></Action></Item>
                    <Item id="Acme.Bad"><Label resid="Refresh.Label"/><Action xsi:type="ShowTaskpane"><SourceLocation resid="Http.Url"/></Action></Item>
                  </Items>
                </Control>
              </Group>
            </CustomTab>
          </ExtensionPoint>
          <ExtensionPoint xsi:type="ContextMenu">
            <OfficeMenu id="ContextMenuText">
              <Control xsi:type="Button" id="Acme.Text"><Label resid="Text.Label"/>
                <Action xsi:type="ShowTaskpane"><SourceLocation resid="Taskpane.Url"/></Action>
              </Control>
            </OfficeMenu>
          </ExtensionPoint>
        </DesktopFormFactor>
      </Host>
    </Hosts>
    <Resources>
      <bt:Urls>
        <bt:Url id="Taskpane.Url" DefaultValue="https://acme.example/taskpane.html"/>
        <bt:Url id="Commands.Url" DefaultValue="https://acme.example/commands.html"/>
        <bt:Url id="Http.Url" DefaultValue="http://acme.example/insecure.html"/>
      </bt:Urls>
      <bt:ShortStrings>
        <bt:String id="Group.Label" DefaultValue="Acme"/>
        <bt:String id="Button.Label" DefaultValue="Show Prices"/>
        <bt:String id="Tab.Label" DefaultValue="Acme"/>
        <bt:String id="Tools.Label" DefaultValue="Tools"/>
        <bt:String id="Menu.Label" DefaultValue="More"/>
        <bt:String id="Refresh.Label" DefaultValue="Refresh All"/>
        <bt:String id="Text.Label" DefaultValue="Price This Text"/>
      </bt:ShortStrings>
      <bt:LongStrings><bt:String id="Button.Tip" DefaultValue="Opens the prices pane."/></bt:LongStrings>
    </Resources>
  </VersionOverrides>
</OfficeApp>"#;

    #[test]
    fn office_manifests_become_plugins() {
        let m = parse_manifest(MANIFEST, "deckcraft").unwrap();
        assert_eq!(m.id, "office.0d5c7a1e-1111-4bcd-9eaf-0123456789ab");
        assert_eq!((m.name.as_str(), m.author.as_str(), m.homepage.as_str()), ("Acme Prices", "Acme & Sons", "https://acme.example/help"));
        assert_eq!(m.apps, ["deckcraft"]);
        let o = m.office.as_ref().unwrap();
        assert_eq!(o.permissions, Permission::ReadWriteDocument);
        assert_eq!(o.requirements, [("PowerPointApi".to_string(), "1.3".to_string())]);
        assert_eq!(o.app_domains, ["https://login.acme.example"]);
        // The default pane, the Home button, the menu's function (the http:// item is dropped) and the text item.
        let ids: Vec<&str> = m.commands.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["taskpane", "Acme.Taskpane", "Acme.Refresh", "Acme.Text"]);
        assert_eq!(
            o.commands["Acme.Taskpane"],
            OfficeCommand::ShowTaskpane { url: "https://acme.example/taskpane.html".into(), taskpane_id: Some("Main".into()) }
        );
        assert_eq!(
            o.commands["Acme.Refresh"],
            OfficeCommand::ExecuteFunction {
                function: "refreshAll".into(),
                url: "https://acme.example/commands.html".into(),
                control: "Acme.Refresh".into()
            }
        );
        assert_eq!(m.ui.ribbon.len(), 2);
        assert_eq!((m.ui.ribbon[0].tab.as_str(), m.ui.ribbon[0].group.as_str()), ("Home", "Acme"));
        assert_eq!(m.ui.ribbon[0].buttons[0].tooltip.as_deref(), Some("Opens the prices pane."));
        assert_eq!((m.ui.ribbon[1].tab.as_str(), m.ui.ribbon[1].buttons[0].label.as_deref()), ("Acme", Some("Refresh All")));
        assert_eq!(m.ui.menus.len(), 1);
        assert_eq!(m.ui.menus[0].menu, MenuKind::Text);
    }

    #[test]
    fn office_commands_open_panes_instead_of_processes() {
        let m = parse_manifest(MANIFEST, "deckcraft").unwrap();
        let id = m.id.clone();
        crate::install(m, std::env::temp_dir()).unwrap();
        let r = run_command(&id, "Acme.Text", serde_json::json!({}), serde_json::json!({})).unwrap();
        assert!(matches!(r.office, Some(OfficeCommand::ShowTaskpane { .. })));
        assert!(r.commands.is_empty());
        assert_eq!(crate::get(&id).unwrap().status(), "ready", "no process was started");
        assert_eq!(crate::get(&id).unwrap().info()["kind"], "office");
        assert!(crate::unload(&id));
    }

    #[test]
    fn foreign_or_broken_office_manifests_are_refused() {
        assert!(parse_manifest(MANIFEST, "gridcraft").unwrap_err().contains("not this app"));
        for bad in [
            "<OfficeApp/>",
            "not xml <<<",
            "<Other/>",
            &MANIFEST.replace("0d5c7a1e-1111-4bcd-9eaf-0123456789ab", "../../etc"),
            &MANIFEST.replace("https://acme.example/", "http://acme.example/"),
        ] {
            assert!(parse_manifest(bad, "deckcraft").is_err(), "{bad:.40}");
        }
        let deep = format!("<OfficeApp>{}{}</OfficeApp>", "<a>".repeat(100), "</a>".repeat(100));
        assert!(parse_manifest(&deep, "deckcraft").unwrap_err().contains("deeply"));
    }

    #[test]
    fn unmet_requirement_sets_block_the_add_in() {
        let xml = MANIFEST.replace("0d5c7a1e-1111-4bcd-9eaf-0123456789ab", "0d5c7a1e-2222-4bcd-9eaf-0123456789ab").replace(
            r#"<Requirements><Sets DefaultMinVersion="1.1"><Set Name="PowerPointApi" MinVersion="1.3"/></Sets></Requirements>"#,
            r#"<Requirements><Sets DefaultMinVersion="1.9"><Set Name="PowerPointApi"/><Set Name="Selection" MinVersion="1.1"/></Sets>
               <Methods><Method Name="Document.getSelectedDataAsync"/><Method Name="Document.getFileAsync"/></Methods></Requirements>"#,
        );
        assert_ne!(xml, MANIFEST);
        let m = parse_manifest(&xml, "deckcraft").unwrap();
        let o = m.office.as_ref().unwrap();
        assert_eq!(
            o.requirements,
            [("PowerPointApi".to_string(), "1.9".to_string()), ("Selection".to_string(), "1.1".to_string())],
            "DefaultMinVersion applies"
        );
        assert_eq!(o.methods, ["Document.getSelectedDataAsync", "Document.getFileAsync"]);
        let id = m.id.clone();
        crate::install(m, std::env::temp_dir()).unwrap();
        let p = crate::get(&id).unwrap();
        assert_eq!(p.state(), "blocked");
        let why: Vec<String> = p.blocked_by().iter().map(|b| b.message.clone()).collect();
        assert_eq!(why, ["Needs PowerPointApi 1.9; DeckCraft supports up to 1.4", "Needs Document.getFileAsync, which DeckCraft doesn't support"]);
        assert!(run_command(&id, "taskpane", serde_json::json!({}), serde_json::json!({})).unwrap_err().contains("blocked"));
        assert!(crate::unload(&id));
    }

    #[test]
    fn sync_actions_read_as_json() {
        let a: Vec<Action> = serde_json::from_str(
            r#"[{"op":"load","path":[{"p":"presentation"},{"m":"getSelectedShapes"}],"props":["items","name"]},
                {"op":"set","path":[{"p":"presentation"},{"p":"slides"},{"m":"getItemAt","a":[0]},{"p":"shapes"},{"m":"getItem","a":["257"]}],"prop":"left","value":72},
                {"op":"call","path":[{"p":"presentation"},{"m":"getSelectedShapes"}],"method":"delete","args":[]}]"#,
        )
        .unwrap();
        assert!(matches!(&a[1], Action::Set { prop, .. } if prop == "left"));
        assert!(matches!(&a[0], Action::Load { path, .. } if path[1] == Step::Method { m: "getSelectedShapes".into(), a: vec![] }));
    }
}

// ---------------------------------------------------------------- requirements (manifest 1.3)

mod requirements {
    use serde_json::json;

    use crate::Manifest;

    fn add(id: &str, version: &str, extra: serde_json::Value) {
        let mut m = json!({"id": id, "name": id.rsplit('.').next().unwrap_or(id), "version": version, "run": {"command": "./not-started"}});
        if let (Some(m), Some(extra)) = (m.as_object_mut(), extra.as_object()) {
            m.extend(extra.clone());
        }
        crate::install(Manifest::parse(&m.to_string()).unwrap(), std::env::temp_dir()).unwrap();
    }

    fn messages(id: &str) -> Vec<String> {
        crate::get(id).unwrap().blocked_by().iter().map(|b| b.message.clone()).collect()
    }

    #[test]
    fn blocked_add_ins_lose_their_commands_until_what_they_need_arrives() {
        add(
            "t.req.reports",
            "1.4.0",
            json!({"commands": [{"id": "go", "label": "Go"}], "events": ["documentSaved"], "macros": {"language": "VBA"},
                   "requires": {"addins": [{"id": "t.req.vba", "version": "^0.1.4", "github": "sirmmo/craft-vba", "why": "Runs the report macros"}]}}),
        );
        let reports = crate::get("t.req.reports").unwrap();
        assert_eq!(reports.state(), "blocked");
        assert!(!reports.active() && reports.enabled());
        assert_eq!(messages("t.req.reports"), ["Needs craft-vba, which isn't installed"]);
        assert!(crate::run_command("t.req.reports", "go", json!({}), json!({})).unwrap_err().contains("blocked"));
        assert!(!crate::listeners("documentSaved").contains(&"t.req.reports".to_string()), "a blocked add-in hears no events");
        assert!(!crate::macro_plugins().iter().any(|(id, _)| id == "t.req.reports"), "nor runs macros");
        let info = reports.info();
        assert_eq!(info["state"], "blocked");
        assert_eq!(info["blockedBy"][0]["kind"], "addin");
        assert_eq!(info["blockedBy"][0]["id"], "t.req.vba");
        assert_eq!(info["requires"]["addins"][0]["github"], "sirmmo/craft-vba");
        assert!(reports.status().starts_with("blocked: "));
        let missing = crate::missing("t.req.reports");
        assert_eq!((missing.len(), missing[0].name.as_str(), missing[0].version.as_str()), (1, "craft-vba", "^0.1.4"));

        // Out of range: still blocked; an update out of range conflicts.
        add("t.req.vba", "0.1.3", json!({}));
        assert_eq!(messages("t.req.reports"), ["Needs vba ^0.1.4 (installed: 0.1.3)"]);
        let newer = Manifest::parse(&json!({"id": "t.req.vba", "name": "vba", "version": "0.2.0", "run": {"command": "x"}}).to_string()).unwrap();
        assert_eq!(crate::conflicts(&newer).len(), 1, "0.2.0 doesn't suit reports either");

        // In range: unblocked without a restart.
        add("t.req.vba", "0.1.5", json!({}));
        assert!(reports.active(), "{:?}", messages("t.req.reports"));
        assert_eq!(reports.state(), "loaded");
        assert!(crate::listeners("documentSaved").contains(&"t.req.reports".to_string()));
        assert_eq!(crate::required_by("t.req.vba"), ["t.req.reports"]);
        assert_eq!(crate::get("t.req.vba").unwrap().info()["requiredBy"], json!(["t.req.reports"]));
        let order = crate::start_order();
        let at = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(at("t.req.vba") < at("t.req.reports"), "{order:?}");
        let c = crate::conflicts(&newer);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].message, "reports needs vba ^0.1.4; 0.2.0 doesn't match");
        let tree = crate::requirements("t.req.reports").unwrap();
        assert_eq!(tree["satisfied"], true);
        assert_eq!((tree["tree"][0]["have"].as_str(), tree["tree"][0]["state"].as_str()), (Some("0.1.5"), Some("loaded")));

        // Turning the required one off blocks the dependent; back on unblocks it.
        assert_eq!(crate::dependents("t.req.vba"), ["t.req.reports"]);
        crate::set_enabled("t.req.vba", false).unwrap();
        assert_eq!(messages("t.req.reports"), ["Needs vba, which is turned off"]);
        crate::set_enabled("t.req.vba", true).unwrap();
        assert!(reports.active());
        // A required add-in that fails for good blocks its dependents too.
        crate::get("t.req.vba").unwrap().fail("flooded the app");
        assert_eq!(messages("t.req.reports"), ["Needs vba, which has failed"]);
        crate::set_enabled("t.req.vba", false).unwrap();
        crate::set_enabled("t.req.vba", true).unwrap();
        assert!(reports.active(), "turning it off and on gives it a fresh start");

        // Unloading it blocks the dependent again.
        assert!(crate::unload("t.req.vba"));
        assert_eq!(messages("t.req.reports"), ["Needs craft-vba, which isn't installed"]);
        let tree = crate::requirements("t.req.reports").unwrap();
        assert_eq!((tree["satisfied"].as_bool(), tree["tree"][0]["state"].as_str()), (Some(false), Some("missing")));
        assert!(crate::unload("t.req.reports"));
    }

    #[test]
    fn app_and_protocol_requirements_are_checked_against_this_host() {
        add(
            "t.req.future",
            "1.0.0",
            json!({"requires": {"apps": {"deckcraft": ">=99.0"}, "protocol": ">=9.0", "features": ["dialogs", "telepathy"]}}),
        );
        let why = messages("t.req.future");
        assert_eq!(why.len(), 3, "{why:?}");
        assert!(why[0].starts_with("Needs DeckCraft 99.0 or later (this is "), "{why:?}");
        assert_eq!(why[1], "Needs plugin protocol 9.0 or later (this DeckCraft has 2.0)");
        assert_eq!(why[2], "Needs `telepathy`, which this DeckCraft doesn't have");
        add(
            "t.req.today",
            "1.0.0",
            json!({"requires": {"apps": {"deckcraft": ">=0.1", "gridcraft": ">=99"}, "protocol": ">=1.2", "features": ["dialogs", "push"]}}),
        );
        assert!(crate::get("t.req.today").unwrap().active());
        assert!(crate::unload("t.req.future") && crate::unload("t.req.today"));
    }

    #[test]
    fn cycles_block_and_unloading_breaks_them() {
        add("t.req.cyc-a", "1.0.0", json!({"requires": {"addins": [{"id": "t.req.cyc-b"}]}}));
        add("t.req.cyc-b", "1.0.0", json!({"requires": {"addins": [{"id": "t.req.cyc-a"}]}}));
        assert_eq!(messages("t.req.cyc-a"), ["Requirement cycle: cyc-a → cyc-b → cyc-a"]);
        assert_eq!(crate::get("t.req.cyc-b").unwrap().blocked_by()[0].kind, crate::requires::Kind::Cycle);
        assert!(crate::unload("t.req.cyc-b"));
        assert_eq!(messages("t.req.cyc-a"), ["Needs t.req.cyc-b, which isn't installed"]);
        assert!(crate::unload("t.req.cyc-a"));
    }
}
