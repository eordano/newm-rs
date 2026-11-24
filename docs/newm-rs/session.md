# Session restore and the usage log

Session restore needs a build with the `canvas` feature; the usage log needs `canvas` and
`telemetry`. [SPEC §6](SPEC.md#6-session) has every rule and constant.

## Session restore

Log out and back in, and every window returns to its tiles. Only a session on a TTY saves and
restores; a nested newm-rs does neither. `canvas { restore-session false }` turns it off, and
`restore-exclude app-id="REGEX"` leaves out apps.

- **What is saved:** per canvas, its output, workspace, zoom, view and focus; per window, its app
  ID, title and tiles (floating windows: position and size); per launch, the command line and
  working directory. The file is `$XDG_STATE_HOME/newm-rs/session.json` (by default
  `~/.local/state/newm-rs/session.json`), mode 0600, written atomically by its own thread a
  couple of seconds after a change.
- **Never saved:** dialogs, windows without an app ID, password and portal prompts, lock screens,
  and apps matching `restore-exclude` ([SPEC §6.3](SPEC.md#63-never-saved) has the list).
- **When:** the `quit` action and the exit dialog save one last time. A compositor stopped by a
  signal (SIGINT, SIGTERM, SIGHUP) does not save, so a teardown never overwrites the session.
- **Restoring:** after `spawn-at-startup`, newm-rs draws a placeholder on every saved tile and
  relaunches each app once, from its desktop file or else the saved command line. Each new window
  takes the placeholder of its app whose saved title fits best, and windows of one app may trade
  places for 90 seconds while their titles settle. When no placeholder is left, or after 20
  seconds, the saved focus, zoom and view return. Apps needed only by floating windows are not
  relaunched.

### Terminals

Terminal windows (Ghostty, Alacritty, kitty, foot, Konsole, WezTerm, GNOME Terminal, Xfce
Terminal, xterm) reattach their sessions: newm-rs finds the tmux client or ssh command running in
each window and relaunches the window in the same terminal running it, in a shell that stays open
afterwards. [SPEC §6.5](SPEC.md#65-terminals) lists each terminal's launch form.

- **tmux:** `tmux new-session -A -s <session>`, which attaches to the session or creates it.
  A terminal that runs a process per window (foot, Alacritty) ties each window to its tmux
  client by that process. Windows that share one terminal process (Ghostty, GNOME Terminal) are
  told apart by title, so tmux's `set-titles` must be on for them. Windows with the same title
  each keep their own session.
- **ssh:** `ssh -t <host> tmux new-session -A -s <session>` when newm-rs knows the remote tmux
  session, else the original ssh command line. To ask a remote tmux for its session, newm-rs
  reuses an open ssh ControlMaster connection and never opens a new one or prompts. Windows to one
  host share one connection, so a restore costs one authentication per host.
- With no command found, the window reopens in the directory its title names, else in your home.

### Browsers

Chromium-family browsers (Chromium, Google Chrome, Brave, Microsoft Edge, Vivaldi) relaunch with
their last session: a bare launch gets `--restore-last-session`. Firefox restores its own session
and gets no flag.

## The usage log

A build with the `telemetry` feature logs what the canvas does, so the layout rules can be judged
on real use. The flake's default package and a plain cargo build leave it out: they start no
writer thread and time no operation. [development.md](development.md#building) says how to turn
it on.

- **Where:** `$XDG_STATE_HOME/newm-rs/usage.jsonl` (by default
  `~/.local/state/newm-rs/usage.jsonl`), one JSON object per line. The file stays on your
  machine: newm-rs has no network code.
- **What:** each canvas operation (open, close, move, resize, swap, drop, gather, send-to-edge,
  undo, redo, cancel, app-grow) with a timestamp, the app ID, the windows' tiles before and after,
  the cost of the rearrangement, the camera, and how long the layout took in microseconds; how a
  new window got its size, how restored windows matched, floating decisions, and a summary of
  each camera or window motion. It records app IDs, never window titles.
- **Rotation:** at 64 MiB the file moves to `usage.jsonl.1`, replacing the older one, so at most
  two files exist.
- **Cost:** its own thread writes the lines from a bounded queue; a full queue drops and counts
  lines and never stalls the compositor. The file takes your umask's permissions.

`contrib/usage-summary.py` summarizes the log with Python's standard library, and
`docs/video/sanitize.py` turns it into the README video's replay data.
