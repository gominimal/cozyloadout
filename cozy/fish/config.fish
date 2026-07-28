# Portable fish config (loadout edition).

status is-interactive; or exit

# --- Aliases ---
if command -q eza
    alias eza 'eza --icons auto --git'
    alias ls eza
    alias la 'eza -a'
    alias ll 'eza -l'
    alias lla 'eza -la'
    alias lt 'eza --tree'
end

if command -q fd
    alias fd 'fd --hidden'
end

# --- Colours: Minimal base16 (see COLOURS.md) ---
# Hex is set explicitly rather than via ANSI names (green, brblack, …) so the
# palette is identical regardless of what the host terminal's 16-colour table
# happens to hold. $MINIMAL_THEME picks the variant; anything other than
# "light" gets the dark one.
if test "$MINIMAL_THEME" = light
    set -g __min_b00 f5f5f5 # default background
    set -g __min_b01 ebebeb # lighter surface
    set -g __min_b02 cccccc # selection
    set -g __min_b03 666666 # comments, dim text
    set -g __min_b04 3d3d3d # secondary text
    set -g __min_b05 141414 # default foreground
    set -g __min_b06 0a0a0a # light foreground (rare)
    set -g __min_b07 000000 # lightest (rare)
    set -g __min_b08 b81414 # red
    set -g __min_b09 8b4a18 # orange
    set -g __min_b0a 7a6200 # yellow
    set -g __min_b0b 317247 # green
    set -g __min_b0c 2d6f76 # cyan
    set -g __min_b0d 2850bd # blue
    set -g __min_b0e 7743b1 # purple
    set -g __min_b0f 754938 # brown
else
    set -g __min_b00 141414
    set -g __min_b01 292929
    set -g __min_b02 3d3d3d
    set -g __min_b03 666666
    set -g __min_b04 8f8f8f
    set -g __min_b05 e0e0e0
    set -g __min_b06 ebebeb
    set -g __min_b07 ffffff
    set -g __min_b08 e93535
    set -g __min_b09 dd8440
    set -g __min_b0a cca300
    set -g __min_b0b 7ec897
    set -g __min_b0c 6bbec7
    set -g __min_b0d 4a7aff
    set -g __min_b0e aa81da
    set -g __min_b0f af715a
end

set -g fish_color_normal $__min_b05
set -g fish_color_command $__min_b0d
set -g fish_color_keyword $__min_b0e
set -g fish_color_quote $__min_b0b
set -g fish_color_redirection $__min_b0c
set -g fish_color_end $__min_b0e
set -g fish_color_error $__min_b08
set -g fish_color_param $__min_b05
set -g fish_color_option $__min_b0c
set -g fish_color_comment $__min_b03
set -g fish_color_operator $__min_b0c
set -g fish_color_escape $__min_b0f
set -g fish_color_autosuggestion $__min_b03
set -g fish_color_cwd $__min_b0b
set -g fish_color_cwd_root $__min_b08
set -g fish_color_user $__min_b0b
set -g fish_color_host $__min_b0d
set -g fish_color_host_remote $__min_b0e
set -g fish_color_status $__min_b08
set -g fish_color_cancel -r
set -g fish_color_history_current --bold
set -g fish_color_valid_path --underline
set -g fish_color_match --background=$__min_b02
set -g fish_color_search_match --background=$__min_b02
set -g fish_color_selection $__min_b05 --bold --background=$__min_b02
set -g fish_pager_color_completion $__min_b05
set -g fish_pager_color_description $__min_b04
set -g fish_pager_color_prefix $__min_b0c --bold
set -g fish_pager_color_progress $__min_b04 --background=$__min_b01
set -g fish_pager_color_selected_background --background=$__min_b02

# --- Terminal surface: OSC 10/11/12 and the base16 ANSI table (OSC 4) ---
# The settings above only cover fish's own output. These hand the same palette
# to the *terminal*, which is the only way to colour the surface behind things
# that don't paint their own background — the prompt, bat, delta, bottom's
# widget text — and it makes the 16-colour table itself on-scheme, so a program
# that still speaks in ANSI names lands in the palette rather than in whatever
# the emulator shipped with.
#
# The ANSI mapping is the standard base16 one tabulated in COLOURS.md. Note what
# it costs: slots 9–14 go to the greyscale ramp and the leftover accents, so a
# program asking for "bright green" gets base01, a dark surface grey. That is
# the usual base16 trade for having all 16 slots on-scheme.
#
# $__MINIMAL_TERM_THEMED is exported, so nested shells and zellij panes inherit
# it and skip both the apply and the reset. Only the outermost fish owns the
# terminal's colours, and it puts them back when it exits — otherwise an inner
# shell exiting would strip the palette out from under the shell still running,
# and sshing into a box with this loadout would leave your local terminal
# recoloured after you logged out.
# Every sequence is terminated with ST written as `\e\x5c`, and only one
# sequence is emitted per printf. Both are deliberate: fish's single quotes are
# not literal — they collapse `\\` to `\` before printf ever sees the string —
# so the natural-looking '\e]10;…\e\\\e]11;…' silently loses an escape and
# prints a stray "e" instead of the second ESC. `\x5c` has no such ambiguity.
if test "$TERM" != dumb; and not set -q __MINIMAL_TERM_THEMED
    set -gx __MINIMAL_TERM_THEMED 1

    printf '\e]10;#%s\e\x5c' $__min_b05 # foreground
    printf '\e]11;#%s\e\x5c' $__min_b00 # background
    printf '\e]12;#%s\e\x5c' $__min_b05 # cursor

    # ANSI 0–15. One sequence per slot rather than a single multi-pair OSC 4:
    # both are legal, but the per-slot form is what every terminal that
    # implements OSC 4 at all accepts.
    set -l __min_ansi \
        $__min_b00 $__min_b08 $__min_b0b $__min_b0a \
        $__min_b0d $__min_b0e $__min_b0c $__min_b05 \
        $__min_b03 $__min_b09 $__min_b01 $__min_b02 \
        $__min_b04 $__min_b06 $__min_b0f $__min_b07
    for __min_i in (seq 16)
        printf '\e]4;%d;#%s\e\x5c' (math $__min_i - 1) $__min_ansi[$__min_i]
    end
    set -e __min_i

    function __min_restore_terminal --on-event fish_exit
        # OSC 110/111/112 reset foreground/background/cursor; OSC 104 with no
        # parameter resets the whole colour table.
        printf '\e]110\e\x5c'
        printf '\e]111\e\x5c'
        printf '\e]112\e\x5c'
        printf '\e]104\e\x5c'
    end
end

# --- Welcome banner ---
# The Minimal mark, in base0D — the same accent helix gives headings, zellij
# gives the active ribbon and starship gives the working directory.
#
# fish's stock fish_greeting function prints $fish_greeting verbatim, newlines
# and escape sequences included, so setting the variable is all this needs. It
# is built here rather than down in Misc because it needs the palette variables
# still in scope — they are erased immediately below. Continuation lines start
# hard against the left margin on purpose: any indent would land inside the art.
set -g fish_greeting (set_color $__min_b0d)"   ████  ████▄
▄▄▄ ▀███▄ ▀███▄
▀███  ▀███  ▀███"(set_color normal)

set -e __min_b00 __min_b01 __min_b02 __min_b03 __min_b04 __min_b05 __min_b06 __min_b07 \
    __min_b08 __min_b09 __min_b0a __min_b0b __min_b0c __min_b0d __min_b0e __min_b0f

# --- zellij auto-start (kept before starship, as in the original) ---
if command -q zellij; and test "$TERM" != dumb
    eval (zellij setup --generate-auto-start fish | string collect)
end

# --- starship prompt ---
if command -q starship; and test "$TERM" != dumb
    starship init fish | source
end

if command -q zoxide
    zoxide init --cmd cd fish | source
end

# --- broot `br` launcher: regenerated from the binary, no shipped script ---
if command -q broot
    broot --print-shell-function fish | source
end

# --- fzf (fuzzy finder) ---
if command -q fzf
    fzf --fish | source

    # zellij claims Ctrl-T (tab mode), so it never reaches fzf inside a
    # session. Move the file picker to Alt-T — zellij ignores it, and it
    # still works in a bare terminal. Ctrl-R (history) and Alt-C (cd) don't
    # collide with zellij's defaults, so they're left as-is.
    bind \et fzf-file-widget

    set -gx FZF_DEFAULT_OPTS '--height 40% --layout=reverse --border'
    if command -q fd
        set -gx FZF_DEFAULT_COMMAND 'fd --hidden --strip-cwd-prefix --exclude .git'
        set -gx FZF_CTRL_T_COMMAND "$FZF_DEFAULT_COMMAND"
        set -gx FZF_ALT_C_COMMAND 'fd --type d --hidden --strip-cwd-prefix --exclude .git'
    end
    command -q bat; and set -gx FZF_CTRL_T_OPTS "--preview 'bat --color=always --style=numbers {}'"
    command -q eza; and set -gx FZF_ALT_C_OPTS "--preview 'eza --tree --color=always --icons {}'"
end

# --- Misc ---
# $fish_greeting is the banner, set up in the colours section above. Clear it
# with `set fish_greeting` here to go back to a silent shell.

function __force_repaint --on-event fish_postexec
    commandline -f repaint
end
