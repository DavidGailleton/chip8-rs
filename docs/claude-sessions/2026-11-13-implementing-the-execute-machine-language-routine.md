# Implementing the Execute machine language routine

*Claude Code session, 2026-11-13. Exported from the local transcript: my prompts and Claude's replies; tool calls are shown as one-line summaries and tool output is omitted.*

---

### 🧑 David

Is it worth to implement the Execute machine language routine ?

> 🔧 **Bash** — Look for how opcode 0NNN is currently handled
> 🔧 **Bash** — Show the decode start for 0x0 opcodes

### 🤖 Claude

No, it isn't worth implementing, and you couldn't do it properly anyway.

**Why not:**
- `0NNN` (SYS addr) told the original COSMAC VIP to jump to native **RCA 1802 machine code** at address `NNN`. To honor it you'd have to emulate a second CPU, the 1802, along with the VIP's memory map and hardware. That's a separate emulator project.
- Almost no ROMs use it. The few that do are "hybrid" VIP programs that mix CHIP-8 with 1802 routines. The standard test ROMs (IBM logo, corax89, Timendus suite) and nearly all games never touch it.
- Every modern interpreter (CHIP-48, SUPER-CHIP, Octo) skips it too. The usual references, like Cowgod's, say to ignore it.

**What to do with it instead.** You already handle it: in `src/chip8.rs:582`, anything `0x0___` that isn't `00E0` or `00EE` falls through to `ExecutionError::UnknownOpcode`. That's the better of the two usual choices:

| Option | Effect |
|---|---|
| **Return an error** (what you do now) | If a ROM hits `0NNN`, you see it right away. In practice that almost always means the PC ran into data or zeroed memory: `0000` is the most common "executing garbage" opcode, and it's usually caused by a bad jump, a broken `00EE` stack, or a wrong `BNNN` quirk. |
| Treat it as a no-op | The emulator keeps going silently, executes garbage, and the real bug shows up somewhere unrelated. |

You can't really argue for the no-op on performance either. The decode cost is the same either way, since it's one more `match` arm, and the compiler turns your nibble match into a jump table or a short compare chain.

If you ever run a VIP hybrid ROM that needs it, you'll know right away because your error message prints the exact opcode. Until then, keep it as an error.
