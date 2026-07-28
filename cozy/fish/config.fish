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
    set -g __min_b01 ebebeb # lighter surface
    set -g __min_b02 cccccc # selection
    set -g __min_b03 666666 # comments, dim text
    set -g __min_b04 3d3d3d # secondary text
    set -g __min_b05 141414 # default foreground
    set -g __min_b08 b81414 # red
    set -g __min_b0b 317247 # green
    set -g __min_b0c 2d6f76 # cyan
    set -g __min_b0d 2850bd # blue
    set -g __min_b0e 7743b1 # purple
    set -g __min_b0f 754938 # brown
else
    set -g __min_b01 292929
    set -g __min_b02 3d3d3d
    set -g __min_b03 666666
    set -g __min_b04 8f8f8f
    set -g __min_b05 e0e0e0
    set -g __min_b08 e93535
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

set -e __min_b01 __min_b02 __min_b03 __min_b04 __min_b05 \
    __min_b08 __min_b0b __min_b0c __min_b0d __min_b0e __min_b0f

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
set fish_greeting

function __force_repaint --on-event fish_postexec
    commandline -f repaint
end
