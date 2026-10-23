# Completion for Jelly in fish: install.sh puts it where fish finds it, or add
# `jelly completions fish | source` to ~/.config/fish/config.fish. Jelly
# itself says what can come next (jelly __complete), so titles complete too.

function __jelly_complete
    set -l typed (commandline -opc)
    jelly __complete $typed[2..-1] (commandline -ct | string unescape) 2>/dev/null
end

complete -c jelly -n '__fish_seen_subcommand_from --play' -F
complete -c jelly -n 'not __fish_seen_subcommand_from --play' -f -a '(__jelly_complete)'
