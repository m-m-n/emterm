// Probe: feed realistic SGR sequences into term_core and dump cell flags.
use term_core::terminal_core::TerminalCore;

fn main() {
    let mut core = TerminalCore::new(80, 24, 100);

    // Row 0: plain text, no SGR at all
    core.process_pty_data(b"plain\r\n");
    // Row 1: indexed red, NOT bold
    core.process_pty_data(b"\x1b[31mred-nobold\x1b[0m\r\n");
    // Row 2: bold + indexed red
    core.process_pty_data(b"\x1b[1;31mred-bold\x1b[0m\r\n");
    // Row 3: 256-color fg
    core.process_pty_data(b"\x1b[38;5;245mgrey245\x1b[0m\r\n");
    // Row 4: truecolor fg
    core.process_pty_data(b"\x1b[38;2;100;150;200mtruecolor\x1b[0m\r\n");
    // Row 5: zsh-style prompt fragment (%F{cyan} renders as 36)
    core.process_pty_data(b"\x1b[36mcyan-prompt\x1b[39m\r\n");

    for row in 0..6u16 {
        let ch = core.get_cell_char(0, row);
        let flags = core.get_cell_flags(0, row);
        let fg = core.get_cell_fg(0, row);
        println!(
            "row={} ch={:?} flags={:#06x} fg={:#010x} (tag={}, idx/r={})",
            row,
            ch,
            flags,
            fg,
            (fg >> 24) & 0xff,
            (fg >> 16) & 0xff
        );
    }
}
