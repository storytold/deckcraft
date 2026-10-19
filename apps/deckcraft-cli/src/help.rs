pub struct Subcommand {
    pub name: &'static str,
    pub usage: &'static str,
    pub summary: &'static str,
    pub details: &'static str,
}

pub const SUBCOMMANDS: &[Subcommand] = &[
    Subcommand {
        name: "info",
        usage: "info FILE",
        summary: "print a presentation's slides, layouts and theme as JSON",
        details: "Prints FILE (.deckcraft or .pptx) as JSON: its slides (index, id, title, layout, how many shapes,
notes, transition), its layouts, theme fonts, sections and slide size.

The shapes of one slide, with their ids, boxes and text, come from the slide.inspect command:

  deckcraft-cli run --in deck.deckcraft --print --cmd 'slide.inspect={\"index\":0}'",
    },
    Subcommand {
        name: "render",
        usage: "render (FILE | --sample) [--slide N | --all] [--scale S] [--edit] [--transparent] OUT",
        summary: "draw slides as PNG images",
        details: "Draws slides of FILE, or of the sample presentation with --sample, as PNG images. FILE comes first;
OUT is the image to write. With --all, OUT is a folder that gets one slide-01.png, slide-02.png… per
slide. Prints each image's path and size.

  --slide N        the slide at index N; indices start at 0, and 0 is the default
  --scale S        pixels per point, 2 by default: a 960×540 slide becomes 1920×1080
  --edit           draw what the editor shows: placeholder prompts and their dashed outlines
  --transparent    leave the slide background out, so only what is on the slide is drawn

  deckcraft-cli render deck.deckcraft --slide 2 slide-3.png",
    },
    Subcommand {
        name: "render-transition",
        usage: "render-transition FILE --slide N --t T[,T...] [--width PX] OUT.png",
        summary: "draw frames of the transition into a slide as PNG images",
        details: "Draws the transition into slide N of FILE at each time T, from 0 (the slide before) to 1 (slide N),
as PNG images. With one T, OUT.png is the image; with several, each frame is written next to it as
OUT-t0.250.png and so on. Prints each image's path, size, transition and time.

  --slide N    the slide the transition leads into; indices start at 0
  --t T,T...   the times to draw, comma-separated
  --width PX   the image width in pixels, twice the slide width in points by default

  deckcraft-cli render-transition deck.deckcraft --slide 1 --t 0.25,0.5,0.75 fade.png",
    },
    Subcommand {
        name: "convert",
        usage: "convert IN OUT",
        summary: "save a presentation in the format OUT's extension names",
        details: "Opens IN (.deckcraft or .pptx) and writes it to OUT, in the format OUT's extension names:
.deckcraft, .pptx, .pdf, .txt or .md for the outline, .png or .jpg for the first slide, .gif for an
animated GIF with one frame per slide. Other extensions are refused. Prints OUT.

  deckcraft-cli convert deck.deckcraft deck.pptx",
    },
    Subcommand {
        name: "run",
        usage: "run [--in FILE | --sample] [--cmd COMMAND[=PARAMS]]... [--save OUT] [--export OUT [--export-params JSON]] [--print]",
        summary: "run commands on a presentation, then save or export it",
        details: "Runs commands, in order, on one presentation: FILE with --in, the sample presentation with --sample,
else a new one with an empty title slide. Then --save writes the presentation: .deckcraft keeps
everything; .pptx, .pdf and .md (the outline) are written as convert writes them. --export writes the
current slide as .png or .jpg, or the presentation as .pdf, .pptx or an animated .gif (one frame per
slide). --export-params passes file.export options as one JSON object, such as
'{\"width\": \"large\", \"secondsPerSlide\": 3, \"loop\": false, \"slides\": [0, 1, 2]}' for a GIF
(`deckcraft-cli describe file.export` lists them).

Each --cmd is one command: COMMAND is a command id, then `=`, then the command's parameters as one
JSON object. Quote it for the shell. A command that needs no parameters is just its id.

  deckcraft-cli run \\
    --cmd 'slide.new={\"layout\":\"titleOnly\",\"title\":\"Q3 results\"}' \\
    --cmd 'shape.insert={\"preset\":\"roundRect\",\"rect\":[80,160,320,120],\"text\":\"Revenue\"}' \\
    --cmd 'format.size={\"size\":28}' \\
    --save q3.deckcraft

`deckcraft-cli commands` lists the command ids; `deckcraft-cli describe COMMAND` explains one and its
parameters.

What the ids don't say:
- Positions and sizes are in points on a 960×540 slide, x to the right and y down; a rect is
  [x, y, width, height].
- Slide indices start at 0. slide.new inserts after the current slide and makes the new one current;
  slide.go {\"index\": N} makes slide N current.
- Shape ids are looked up on the current slide only.
- A command that adds a shape (shape.insert, insert.textBox, insert.table…) selects it. format.* acts
  on the selected text or shapes; edit.select {\"ids\": [...]} selects others. Many shape commands
  take the ids directly; `deckcraft-cli describe COMMAND` says what a command acts on.

If a command fails, run stops there and saves nothing. --print shows each command's answer, such as
the id of an inserted shape.",
    },
    Subcommand {
        name: "commands",
        usage: "commands [FILTER]",
        summary: "list the commands run can use, one per line",
        details: "Lists the commands that run --cmd, app and the MCP server take, one per line: the id, its name in
the app and its parameters. FILTER keeps the commands whose id or name contains it, ignoring case.

  deckcraft-cli commands format.

In the parameters, `?` marks an optional one, a spaced ` | ` separates alternatives and `→` shows what
the command answers. `deckcraft-cli describe COMMAND` spells one command out.",
    },
    Subcommand {
        name: "describe",
        usage: "describe COMMAND",
        summary: "explain one command and its parameters",
        details: "Explains one command: what it is in the app, each of its parameters, what it answers, and how run
calls it.

  deckcraft-cli describe shape.insert",
    },
    Subcommand {
        name: "app",
        usage: "app [--port PORT] COMMAND [PARAMS] | --method METHOD [PARAMS]",
        summary: "run a command in the DeckCraft app started with --control",
        details: "Sends one request to the DeckCraft app started with `deckcraft --control PORT` (port 7979 by default)
and prints its answer as JSON. COMMAND [PARAMS] runs a command, as run --cmd does; --method METHOD
[PARAMS] calls a control-channel method such as ui.screenshot. PARAMS is a JSON object.

  deckcraft-cli app shape.insert '{\"preset\":\"star5\",\"rect\":[100,100,200,200]}'
  deckcraft-cli app --method ui.screenshot '{\"path\":\"/tmp/shot.png\"}'",
    },
    Subcommand {
        name: "mcp",
        usage: "mcp [--connect PORT] [--sample] [--automation-read-root DIR] [--automation-write-root DIR]",
        summary: "serve the commands to an agent over MCP on stdio",
        details: "Runs a Model Context Protocol server on stdin and stdout. It works on its own presentation without a
window, or with --connect PORT (or host:port) on the DeckCraft app started with --control PORT.
--sample opens the sample presentation.

--automation-read-root DIR and --automation-write-root DIR confine every file the agent's commands read
(open, pictures, audio, video) and write (save, export, PDF) to those folders, links followed; a root left
out grants none of its access. They run the in-process server, so they conflict with --connect (a running
app is not confined by them).",
    },
    Subcommand {
        name: "bench",
        usage: "bench (FILE | --sample) [--runs N] [--json]",
        summary: "time opening, drawing, saving and exporting a presentation",
        details: "Times how long FILE, or the sample presentation with --sample, takes to open, to draw each slide, to
save as .deckcraft and .pptx, to reopen as .pptx and to export as .pdf, and the peak memory. Prints the
median and slowest of N runs (5 by default, 1 to 1000) as a table, or as JSON with --json.

  deckcraft-cli bench deck.pptx --runs 10",
    },
    Subcommand {
        name: "links",
        usage: "links",
        summary: "print the DeckCraft community and project links",
        details: "Prints the DeckCraft community and project links.",
    },
    Subcommand { name: "version", usage: "--version", summary: "print the version", details: "Prints the version of deckcraft-cli." },
];

pub fn page(name: &str) -> Option<String> {
    let name = match name {
        "help" | "--help" | "-h" => return Some(overview()),
        "--version" | "-V" => "version",
        name => name,
    };
    SUBCOMMANDS.iter().find(|s| s.name == name).map(|s| format!("usage: deckcraft-cli {}\n\n{}", s.usage, s.details))
}

pub fn overview() -> String {
    let width = SUBCOMMANDS.iter().map(|s| s.usage.split(' ').next().unwrap_or_default().len()).max().unwrap_or_default();
    let list: Vec<String> =
        SUBCOMMANDS.iter().map(|s| format!("  {:<width$}  {}", s.usage.split(' ').next().unwrap_or_default(), s.summary)).collect();
    format!(
        "deckcraft-cli works on DeckCraft presentations (.deckcraft, .pptx) without a window.

{}

A presentation is edited by running commands on it:

  deckcraft-cli run --cmd 'slide.new={{\"title\":\"Q3 results\"}}' --save q3.deckcraft

`deckcraft-cli help SUBCOMMAND`, or `deckcraft-cli SUBCOMMAND --help`, explains one subcommand;
`deckcraft-cli help run` explains how to write a command.",
        list.join("\n")
    )
}
