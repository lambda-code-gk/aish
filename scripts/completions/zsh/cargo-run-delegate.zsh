# `cargo run -p ai|aish|aibe -- …` 向け Tab 補完委譲（zsh）。
#
# ~/.zshrc 例:
#   source /path/to/aish/scripts/completions/zsh/cargo-run-delegate.zsh
#   eval "$(aish complete zsh)"
#   eval "$(ai complete zsh)"
#   eval "$(aibe complete zsh)"

_aish_workspace_cargo_complete() {
  local -a words
  words=("${words[@]:1}" "$words[CURRENT]")

  local crate=""
  local i=1
  while (( i <= ${#words[@]} )); do
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
  i=1
  while (( i <= ${#words[@]} )); do
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
  forwarded=("${words[@]:dash_idx+1}")

  local candidates
  candidates="$(
    _CLAP_COMPLETE="${BUFFER}" \
    _CLAP_COMPLETE_BIN="$crate" \
    _CLAP_COMPLETE_ARGS="${forwarded[*]}" \
    cargo run -q -p "$crate" -- "${forwarded[@]}" 2>/dev/null
  )" || return 0

  if [[ -n $candidates ]]; then
    compadd -- ${(f)candidates}
  fi
}

compdef _aish_workspace_cargo_complete cargo
