# maxicom completion for bash (MAX-294): maxicom answers each Tab itself.
# Load it with: eval "$(maxicom --completions bash)" in ~/.bashrc, or save it as
# ~/.local/share/bash-completion/completions/maxicom with bash-completion.
#
# How it works: on Tab, bash calls _maxicom with the line in COMP_LINE and the cursor in
# COMP_POINT; the function calls `maxicom --complete -- <words before> <word typed>` and
# puts what to offer in the COMPREPLY array, which bash shows or inserts. maxicom's reply:
#   values               first line: values to offer follow, one per line
#   <value><TAB><help>   (bash has no place for the help: it is dropped)
# or
#   files<TAB><prefix>   first line only: let bash complete a path
_maxicom() {
    # the words up to the cursor, split at the spaces only: bash's own split cuts at = and :
    # ${COMP_LINE:0:COMP_POINT} is the substring from 0, COMP_POINT characters long: what is
    # left of the cursor. `local` keeps the variables inside the function
    local line=${COMP_LINE:0:COMP_POINT} cur=
    # -a: arrays
    local -a words reply
    # split $line at the spaces into the array words (-a); -r keeps backslashes as they are;
    # <<< feeds a string as the input
    read -r -a words <<< "$line"
    # the line doesn't end with a space: the last word is the one being typed, move it from
    # words to cur. Ending with a space, cur stays empty: a new word is started
    if [[ $line != *[[:space:]] ]]; then
        # ${#words[@]} is the count of words, so this is the last one
        cur=${words[${#words[@]}-1]}
        # the quotes stop bash from expanding the [...] as a file pattern
        unset 'words[${#words[@]}-1]'
    fi
    # IFS says where $( ) output is cut into array items: at newlines only, so a value with
    # spaces stays one item. local: only for this function
    local IFS=$'\n'
    # ${words[@]:1} is every word but the first (the program name); 2>/dev/null hides any
    # error; ( ) around $( ) makes an array of the lines
    reply=($(maxicom --complete -- "${words[@]:1}" "$cur" 2>/dev/null))
    COMPREPLY=()
    # files, or no value: bash completes the paths itself (-o default)
    # `test || return`: anything but "values" ends here with COMPREPLY empty, and the -o
    # default of the `complete` line below makes bash fall back to file names
    [[ ${reply[0]} == values ]] || return 0
    # each value replaces readline's word: the end of ours, after its last = or : (the
    # characters of COMP_WORDBREAKS)
    # e.g. typing --profile=es: readline's word is only `es`, so the value
    # `--profile=esp32` must be given as `esp32`. cut is where readline's word starts in cur
    local cut=0 i value
    # for each character of cur (${cur:i:1} is the one at i): when it is in COMP_WORDBREAKS,
    # readline's word starts after it
    for (( i = 0; i < ${#cur}; i++ )); do
        [[ $COMP_WORDBREAKS == *"${cur:i:1}"* ]] && cut=$(( i + 1 ))
    done
    # each line after the "values" one (index 0)
    for (( i = 1; i < ${#reply[@]}; i++ )); do
        # %%$'\t'* removes the longest end starting with a tab: the help goes, the value stays
        value=${reply[i]%%$'\t'*}
        # the value from character cut on: readline's part of it
        value=${value:cut}
        # nothing left (the value was all before the cut): skip it
        [[ -n $value ]] || continue
        # %q quotes the value for the shell (a space becomes "\ "), -v writes it back to value
        printf -v value '%q' "$value"
        COMPREPLY+=("$value")
    done
}
# register: -F names the function for maxicom; -o bashdefault -o default: when COMPREPLY is
# empty, bash's own completions then file names, which is how the files reply works
complete -o bashdefault -o default -F _maxicom maxicom
