# Chip8-rs minifb Xlib window error on NixOS

*Claude Code session, 2026-11-11. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

`/nixos` dgaillet@laptop-d ~/D/chip8-rs (main)> cargo run -- test_suite/1-chip8-logo.ch8
   Compiling chip8-rs v0.1.0 (~/Documents/chip8-rs)
warning: unused variable: `color`
  --> src/frontend.rs:27:25
   |
27 |             for (pixel, color) in row.iter_mut().zip(0..WIDTH) {
   |                         ^^^^^ help: if this is intentional, prefix it with an underscore: `_color`
   |
   = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: `chip8-rs` (lib) generated 1 warning (run `cargo fix --lib -p chip8-rs` to apply 1 suggestion)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.88s
     Running `target/debug/chip8-rs test_suite/1-chip8-logo.ch8`
error: Failed to create window: failed to load Xlib: OpenError { kind: Library, detail: "opening library failed (libX11.so.6: cannot open shared object file: No such file or directory); opening library failed (libX11.so: cannot open shared object file: No such file or directory)" } How to make this programm runnable. minifb should let window start on wayland

> 🔧 **Bash** — Inspect Cargo config, project files, session type
> 🔧 **Bash** — Check minifb features and backend selection
> 🔧 **Bash** — Find which shared libs minifb dlopens
> 🔧 **Write** — ~/Documents/chip8-rs/flake.nix

### 🤖 Claude

Flakes only see files git knows about, so I'm staging it and then running a quick test with a timeout:

> 🔧 **Bash** — Run emulator inside the new dev shell for 8 seconds
> 🔧 **Bash** — Capture emulator output before killing it

### 🤖 Claude

The emulator runs now. Inside the new dev shell, `cargo run -- test_suite/1-chip8-logo.ch8` starts with no window error and keeps going until it's stopped. I only checked that from the terminal output, so I couldn't see the window or confirm the logo draws correctly.

**Why it failed:** minifb tries Wayland first and only falls back to X11 if that fails. It doesn't link the windowing libraries when it builds. It looks for them by name when the program starts (`libwayland-client.so`, `libxkbcommon.so`, then `libX11.so`). NixOS has no shared `/usr/lib` folder where they'd normally be found, so the Wayland attempt failed silently, X11 failed next, and you only saw the X11 error. Your code is fine; it just couldn't find the libraries.

**Fix:** I added `flake.nix` with a dev shell. It provides the Rust tools and sets `LD_LIBRARY_PATH` (the extra places Linux searches for these libraries) to point at wayland, libxkbcommon and the X11 libraries, which are kept as a fallback.

```sh
nix develop          # enter the shell once
cargo run -- test_suite/1-chip8-logo.ch8
```

Or in one line: `nix develop -c cargo run -- <rom>`.

**Notes:**
- I staged `flake.nix` with `git add` because flakes ignore files git doesn't know about. Running it also created `flake.lock`, which you should commit too. I haven't committed anything.
- The shell uses nixpkgs' `rustc`/`cargo`. If you'd rather keep your rustup toolchain, remove those packages from `packages` and keep only the `LD_LIBRARY_PATH` line.
- If you install `nix-direnv` and add a `.envrc` containing `use flake`, the shell loads automatically when you `cd` into the project.
- The remaining warning, `color` unused at `src/frontend.rs:27`, is in your code, so I've left it alone. As you've probably guessed, zipping with `0..WIDTH` gives you the column index, not a color.

### 🧑 David

i have this warning : 

```text

dgaillet@laptop-d ~/D/chip8-rs (main)> nix develop
warning: Git tree '~/Documents/chip8-rs' is dirty
evaluation warning: The xorg package set has been deprecated, 'xorg.libX11' has been renamed to 'libx11'
evaluation warning: The xorg package set has been deprecated, 'xorg.libXcursor' has been renamed to 'libxcursor'
evaluation warning: The xorg package set has been deprecated, 'xorg.libXrandr' has been renamed to 'libxrandr'
evaluation warning: The xorg package set has been deprecated, 'xorg.libXi' has been renamed to 'libxi'

```


Should not it be better to be available in golbal config ?

> 🔧 **Bash** — Rename deprecated xorg packages and re-evaluate shell

### 🤖 Claude

I fixed the deprecation warnings. nixpkgs renamed the `xorg.*` packages, so the flake now uses `libx11`, `libxcursor`, `libxrandr` and `libxi`, and `nix develop` evaluates without them. The "Git tree is dirty" line is only a notice that you have uncommitted changes. It goes away once you commit.

**Global config vs. the project flake:** I'd keep it in the flake.

- **Installing the libraries globally isn't enough.** Adding `wayland` or `libxkbcommon` to `environment.systemPackages` doesn't make them findable at runtime. NixOS has no shared folder like `/usr/lib` where programs look for libraries, so your binary still wouldn't find them. You'd have to set `LD_LIBRARY_PATH` for your whole session, and every program you run would then pick up those library versions. That can break other apps in ways that are hard to trace back to this.
- **`programs.nix-ld` doesn't help here either.** It's meant for programs built outside Nix that expect a standard Linux layout. Your binary is built with Nix's own toolchain, so nix-ld doesn't apply to it.
- **The flake is the reproducible option.** Anyone who clones the repo, or you on another machine, runs `nix develop` and gets the same toolchain and libraries. Your system config also stays clean of per-project dependencies.

What does belong in your global config, so you don't have to type `nix develop` each time, is **direnv**. Add it to `home.nix`:

```nix
programs.direnv = {
  enable = true;
  nix-direnv.enable = true;
};
```

Then put a `.envrc` in the repo containing `use flake` and run `direnv allow` once. The dev shell will load automatically whenever you `cd` into the project, and the libraries stay scoped to it.

### 🧑 David

add the .envrc and commit the flake

> 🔧 **Bash** — Create .envrc, ignore .direnv, commit flake files

### 🤖 Claude

I committed the flake setup on `main` as "Add Nix flake dev shell and direnv config". It contains four files:

- `flake.nix` and `flake.lock`
- `.envrc`, which contains `use flake`
- `.gitignore`, where I added `.direnv/`, the folder direnv uses to store its cache

Your other work in progress (the `src/` changes, `Cargo.*` and `test_suite/`) is untouched and still uncommitted. Nothing has been pushed.

To make the shell load automatically when you `cd` into the project:
1. Enable `programs.direnv` with `nix-direnv` in `home.nix`, then rebuild.
2. Run `direnv allow` once in the repo.

### 🧑 David

add direnv to my home.nix

> 🔧 **Bash** — Locate home.nix and check existing direnv config
> 🔧 **Bash** — Read end of home.nix and repo state
> 🔧 **Bash** — Check how fish config is managed
> 🔧 **Bash** — Show fish config contents
> 🔧 **Bash** — Check home.nix function arguments
> 🔧 **Edit** — /etc/nixos/home/<user>/home.nix

### 🤖 Claude

Now I'll check that the system configuration builds, without switching to it:

> 🔧 **Bash** — Build NixOS config to validate the change

### 🤖 Claude

I added direnv to `/etc/nixos/home/<user>/home.nix`, and your full system configuration builds without errors. It isn't active yet because switching needs sudo. Run these yourself:

```sh
sudo nixos-rebuild switch --flake /etc/nixos#laptop-d
cd ~/Documents/chip8-rs && direnv allow
```

Open a new terminal before the `cd`, because the shell you're in already started without the direnv hook. From then on, `cd`-ing into chip8-rs loads the dev shell automatically.

What I added:
- **`programs.direnv` with `nix-direnv`:** nix-direnv caches the dev shell, so it only rebuilds when `flake.nix` or `flake.lock` changes, and the cached shell isn't deleted when you clean up the Nix store.
- **The fish hook as `~/.config/fish/conf.d/direnv.fish`:** fish isn't managed by Home Manager on your setup, so the usual `enableFishIntegration` option would have done nothing. Fish loads files in `conf.d/` on its own, so your `config.fish` stays as it is.

Things to know:
- I deleted the `/etc/nixos/result` link. That was the output link from my test build, which had replaced the one left by your previous build. It's a convenience link, not part of your config, and the switch doesn't need it.
- I didn't commit anything in `/etc/nixos`. The `home.nix` change is in the working tree for you to review and commit. I only marked the file so the test build could see the change; nothing is staged.

### 🧑 David

commit the home.nix change

> 🔧 **Bash** — Check /etc/nixos working tree
> 🔧 **Bash** — Commit home.nix direnv change

### 🤖 Claude

I'll also note two lasting gotchas in your memory vault: on NixOS, minifb needs its libraries on `LD_LIBRARY_PATH`, and fish isn't managed by Home Manager.

> 🔧 **Bash** — Inspect L1 memory format
> 🔧 **Bash** — Save L1 memory note about NixOS dlopen and fish hook

### 🤖 Claude

I committed the `home.nix` change in `/etc/nixos` as `dd97456 home: enable direnv with nix-direnv and fish hook`. Nothing else is uncommitted there, and nothing has been pushed.

I also saved a note in your memory folder at `~/.claude/projects/-home-dgaillet-obsidian-vault/memory/nixos-dlopen-devshell.md`. It records three gotchas from this session:
- On NixOS, crates that load system libraries at runtime, like minifb, need a flake dev shell that sets `LD_LIBRARY_PATH`.
- Fish isn't managed by Home Manager, so shell hooks go in `conf.d`.
- nixpkgs renamed the `xorg.*` packages.

The config still needs to be applied: run `sudo nixos-rebuild switch --flake /etc/nixos#laptop-d`, open a new terminal, then run `direnv allow` in chip8-rs.

### 🧑 David

push the nixos config

> 🔧 **Bash** — Check remote and unpushed commits

### 🤖 Claude

It will push the pending commits.

> 🔧 **Bash** — Push nixos config to GitHub

### 🤖 Claude

I pushed the NixOS config to its private repository.

The direnv change still isn't active on this machine until you run `sudo nixos-rebuild switch --flake /etc/nixos#laptop-d`.
