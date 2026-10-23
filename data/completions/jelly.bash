# Completion for Jelly in bash: install.sh puts it where bash-completion finds
# it, or add `source <(jelly completions bash)` to ~/.bashrc. Jelly itself
# says what can come next (jelly __complete), so titles complete too.

_jelly() {
    local cur prev words cword
    # The words as typed, not split at a title's colon
    if declare -F _comp_get_words >/dev/null; then
        _comp_get_words -n : cur prev words cword
    elif declare -F _get_comp_words_by_ref >/dev/null; then
        _get_comp_words_by_ref -n : cur prev words cword
    else
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
        cur=${COMP_WORDS[COMP_CWORD]}
        prev=${COMP_WORDS[COMP_CWORD - 1]}
    fi
    if [[ $prev == --play ]]; then
        compopt -o filenames 2>/dev/null
        mapfile -t COMPREPLY < <(compgen -f -- "$cur")
        return
    fi
    # Without the backslashes typed before spaces and quotes
    local typed=("${words[@]:1:cword}") word
    COMPREPLY=()
    while IFS=$'\t' read -r word _; do
        COMPREPLY+=("$(printf '%q' "$word")")
    done < <("${words[0]/#\~/$HOME}" __complete "${typed[@]//\\/}" 2>/dev/null)
    # Of a title with a colon, bash replaces what's after the colon
    if declare -F _comp_ltrim_colon_completions >/dev/null; then
        _comp_ltrim_colon_completions "$cur"
    elif declare -F __ltrim_colon_completions >/dev/null; then
        __ltrim_colon_completions "$cur"
    fi
}

complete -F _jelly jelly
