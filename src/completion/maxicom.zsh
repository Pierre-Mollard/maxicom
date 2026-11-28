#compdef maxicom
# maxicom completion for zsh (MAX-294): maxicom answers each Tab itself.
# Save it as _maxicom in a folder of $fpath, or: eval "$(maxicom --completions zsh)" in
# ~/.zshrc after compinit.
#
# How it works: the first line (#compdef) tells compinit this file completes maxicom. On
# Tab, zsh calls _maxicom with the line already split: $words (array, from 1 in zsh), the
# index of the word being typed in $CURRENT, and its part left of the cursor in $PREFIX. The
# function calls `maxicom --complete -- <words before> <word typed>` and hands the reply to
# zsh's helpers (_describe for values, _files for paths). maxicom's reply:
#   values               first line: values to offer follow, one per line
#   <value><TAB><help>   (zsh shows the help next to the value)
# or
#   files<TAB><prefix>   first line only: let zsh complete a path
_maxicom() {
    local -a reply described
    local entry value
    # inside out:
    #   words[2,CURRENT-1]   the words between the program name and the one being typed
    #   (@Q)                 removes the quoting the user typed (a\ b is given as `a b`),
    #                        with @ each word stays its own argument
    #   (Q)PREFIX            same for the word being typed
    #   (@f)                 cuts the output into one array item per line
    reply=("${(@f)$(maxicom --complete -- "${(@Q)words[2,CURRENT-1]}" "${(Q)PREFIX}" 2>/dev/null)}")
    # zsh arrays start at 1: reply[1] is the first line
    if [[ ${reply[1]} == files* ]]; then
        # a path glued to its option: the shell's paths after the =
        # compset -P '*=' moves `--log=` out of the word zsh completes, so _files only
        # sees what comes after it
        [[ ${reply[1]} == *= ]] && compset -P '*='
        # zsh's own path completion
        _files
        return
    fi
    # anything else than values: return 1 says "no completion"
    [[ ${reply[1]} == values ]] || return 1
    # every line after the first; build the "value:help" items _describe reads
    for entry in "${(@)reply[2,-1]}"; do
        # %%$'\t'* removes from the tab to the end: the value alone
        value=${entry%%$'\t'*}
        # _describe cuts value:help at the first ':', so a ':' in the value is escaped
        value=${value//:/\\:}
        if [[ $entry == *$'\t'* ]]; then
            # #*$'\t' removes up to the tab: the help alone
            described+=("$value:${entry#*$'\t'}")
        else
            described+=("$value")
        fi
    done
    # in maxicom's order
    # -V: keep the order (zsh sorts otherwise); -t values: the tag (zstyle can use it);
    # `value` the group's name, described the array of items
    _describe -V -t values value described
}

# the file is loaded two ways. From $fpath, compinit runs this file as the body of a
# function named _maxicom: funcstack[1] (the function running) is _maxicom, so complete
# now. Through eval, it isn't: register the function for maxicom instead
if [[ ${funcstack[1]} == _maxicom ]]; then
    _maxicom "$@"
else
    compdef _maxicom maxicom
fi
