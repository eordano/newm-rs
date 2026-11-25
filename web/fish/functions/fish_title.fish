function fish_title
    set -l cmd $argv[1]
    if not set -q argv[1]
        set cmd (status current-command)
        test "$cmd" = fish; and set cmd
    end
    set -l host
    if set -q SSH_TTY; and not set -q TMUX
        set host (prompt_hostname):
    end
    echo -- $host (string sub -l 40 -- $cmd) (prompt_pwd)
end
