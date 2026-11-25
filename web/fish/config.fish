# The demo shell's look: the author's fish prompt (bobthefish), greeting, window titles and
# command timestamps. demo.nix puts bobthefish and functions/ on fish_function_path first.

set fish_greeting
bind \t complete

# bobthefish, as the author sets it; the guest has no Nerd Font, so plain glyphs.
set -g theme_display_git yes
set -g theme_display_git_default_branch no
set -g theme_nerd_fonts no
set -g theme_powerline_fonts no
set -g theme_newline_cursor yes
set -g theme_color_scheme solarized
set -g theme_date_format "+%H:%M:%S"

# The terminal learns the working directory.
function __update_cwd_osc --on-variable PWD --description 'Notify terminal of CWD'
    if status is-command-substitution
        return
    end
    printf \e\]7\;file://localhost%s\e\\ (string escape --style=url -- $PWD)
end
__update_cwd_osc

# When a command started, at the right of its prompt line.
function __print_cmd_timestamp --on-event fish_preexec
    set -l cmd $argv[1]
    set -g __cmd_start_time (date "+%H:%M:%S.%3N")
    set -l newlines (string match -ar '\n' -- $cmd | count)

    set -l prompt_len 40
    set -l cmd_lines (string split '\n' -- $cmd)
    set -l first_line_len (math (string length -- $cmd_lines[1]) + $prompt_len)
    set -l wrapped (math --scale=0 "max(0, ($first_line_len - 1) / $COLUMNS)")
    for line in $cmd_lines[2..-1]
        set -l len (string length -- $line)
        set wrapped (math "$wrapped + "(math --scale=0 "max(0, ($len - 1) / $COLUMNS)"))
    end

    set -l lines_up (math "2 + $newlines + $wrapped")
    set -l ts_display "[$__cmd_start_time]"
    set -l ts_len (string length $ts_display)
    set -l col (math $COLUMNS - $ts_len + 1)

    printf '\e[%dA\e[%dG' $lines_up $col
    set_color brblack
    printf '%s' $ts_display
    set_color normal
    printf '\e[%dB\r' $lines_up
end

# How long a command took, when it took over a second.
function __print_cmd_duration --on-event fish_postexec
    if test $CMD_DURATION -gt 1000
        set -l ms (math "$CMD_DURATION % 1000")
        set -l total_secs (math --scale=0 "$CMD_DURATION / 1000")
        set -l secs (math "$total_secs % 60")
        set -l mins (math --scale=0 "$total_secs / 60")

        set -l end_time (date "+%H:%M:%S.%3N")
        set -l display (printf "+%02d:%02d.%03ds -> [%s]" $mins $secs $ms $end_time)
        set -l len (string length $display)
        set -l col (math $COLUMNS - $len + 1)

        printf '\e[1A\e[%dG' $col
        set_color brblack
        printf '%s' $display
        set_color normal
        printf '\e[1B\r'
    end
end
