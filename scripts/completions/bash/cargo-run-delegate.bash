#!/usr/bin/env bash
# `cargo run -p ai|aish|aibe -- …` 向け Tab 補完委譲（bash）。
#
# ~/.bashrc 例:
#   source /path/to/aish/scripts/completions/bash/cargo-run-delegate.bash
#   source <(aish complete bash)
#   source <(ai complete bash)
#   source <(aibe complete bash)

_aish_workspace_cargo_complete() {
    local cur prev words cword
    _init_completion -n "=:" || return

    local crate=""
    local i=0
    while (( i < cword )); do
        if [[ ${words[i]} == -p && -n ${words[i+1]:-} ]]; then
            crate="${words[i+1]}"
            break
        fi
        ((i++))
    done

    case "$crate" in
        aish|ai|aibe) ;;
        *) return 0 ;;
    esac

    local dash_idx=-1
    i=0
    while (( i < cword )); do
        if [[ ${words[i]} == -- ]]; then
            dash_idx=$i
            break
        fi
        ((i++))
    done
    if (( dash_idx < 0 )); then
        return 0
    fi

    local -a forwarded
    forwarded=("${words[@]:dash_idx+1:cword-dash_idx}")
    forwarded+=("$cur")

    local candidates
    candidates="$(
        _CLAP_COMPLETE=${COMP_LINE} \
        _CLAP_COMPLETE_BIN="$crate" \
        _CLAP_COMPLETE_ARGS="${forwarded[*]}" \
        cargo run -q -p "$crate" -- "${forwarded[@]}" 2>/dev/null
    )" || return 0

    if [[ -n $candidates ]]; then
        COMPREPLY=( $(compgen -W "$candidates" -- "$cur") )
    fi
}

complete -F _aish_workspace_cargo_complete cargo
