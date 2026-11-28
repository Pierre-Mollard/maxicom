# maxicom completion for fish (MAX-294): maxicom answers each Tab itself.
# Save it with: maxicom --completions fish > ~/.config/fish/completions/maxicom.fish
#
# How it works: the `complete` line at the end tells fish to run __maxicom_complete on Tab
# after maxicom; each line the function prints is a candidate (value<TAB>help: fish shows
# the help). The function calls `maxicom --complete -- <words before> <word typed>`.
# maxicom's reply:
#   values               first line: values to offer follow, one per line
#   <value><TAB><help>   printed as they are: it is already fish's format
# or
#   files<TAB><prefix>   first line only: complete a path, after <prefix> (e.g. --log=)
function __maxicom_complete
    # commandline -opc: the words (-o tokens, quotes removed) before the cursor (-c) of the
    # current command (-p); [2..-1] drops the program name. ( ) is fish's $( ). fish arrays
    # start at 1. set -l: a local variable
    set -l before (commandline -opc)[2..-1]
    # -ct: the word under the cursor, the one being typed
    set -l current (commandline -ct)
    # each output line becomes one item of reply; 2>/dev/null hides any error
    set -l reply (maxicom --complete -- $before $current 2>/dev/null)
    # cut the first line once (-m1) at the tab: kind[1] is files or values, kind[2] the
    # prefix of files
    set -l kind (string split -m1 \t -- $reply[1])
    if test "$kind[1]" = files
        # a path glued to its option: the paths after it
        set -l prefix $kind[2]
        # what was typed after the prefix: the substring from (prefix length + 1)
        set -l path (string sub -s (math (string length -- "$prefix") + 1) -- $current)
        # fish's own helper, the paths starting with $path; glue the prefix back on each
        for found in (__fish_complete_path "$path")
            echo "$prefix$found"
        end
        return
    end
    # `; and` runs the printf only when the test passed: every line after the first
    test "$kind[1]" = values; and printf '%s\n' $reply[2..-1]
end

# -c maxicom: for this command; -f: no file names besides ours (the function gives the
# paths itself); -k: keep the order given; -a: the candidates, from running the function
complete -c maxicom -f -k -a '(__maxicom_complete)'
