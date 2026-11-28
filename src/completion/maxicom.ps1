# maxicom completion for PowerShell (MAX-294): maxicom answers each Tab itself.
# Load it with: maxicom --completions powershell | Out-String | Invoke-Expression
# in your $PROFILE.
#
# How it works: Register-ArgumentCompleter gives PowerShell a script block to run on Tab
# for maxicom (-Native: an outside program, not a PowerShell command). It gets the line
# parsed, calls `maxicom --complete -- <words before> <word typed>` and returns
# CompletionResult objects. maxicom's reply:
#   values               first line: values to offer follow, one per line
#   <value><TAB><help>   (the help shows as a tooltip / in the menu)
# or
#   files<TAB><prefix>   first line only: let PowerShell complete a path
Register-ArgumentCompleter -Native -CommandName maxicom, maxicom.exe -ScriptBlock {
    # what PowerShell passes: the word under the cursor, the whole command parsed (an AST),
    # and the cursor's offset in the line
    param($wordToComplete, $commandAst, $cursorPosition)
    # the words before the one being typed, the program's name left out
    # CommandElements are the words; Skip 1 drops the program; keep those that end before
    # the cursor; Extent.Text is a word's text as typed. @( ) makes sure it is an array,
    # even of 0 or 1 word. | passes objects along, $_ is the current one
    $before = @($commandAst.CommandElements | Select-Object -Skip 1 |
        Where-Object { $_.Extent.EndOffset -lt $cursorPosition } |
        ForEach-Object { $_.Extent.Text })
    # before 7.3, an empty argument never reaches a native program
    # so the word being typed is given as "" there, which maxicom reads as empty
    $current = $wordToComplete
    if ($current -eq '' -and $PSVersionTable.PSVersion -lt [version]'7.3') { $current = '""' }
    # & runs the program; @before spreads the array as separate arguments; 2>$null hides
    # errors; the output is one array item per line
    $reply = @(& maxicom --complete -- @before $current 2>$null)
    # files, or no value: PowerShell completes the paths itself
    # returning nothing makes PowerShell fall back to its file names
    if ($reply.Count -eq 0 -or $reply[0] -ne 'values') { return }
    # every line after the first
    $reply | Select-Object -Skip 1 | ForEach-Object {
        # cut at the first tab ("`t" is a tab: ` is PowerShell's escape), into two variables
        $value, $help = $_ -split "`t", 2
        # a tooltip can't be empty: the value then
        if (-not $help) { $help = $value }
        # the text inserted in the line: in single quotes when the value holds a space or a
        # character PowerShell reads specially ('' is a quote inside single quotes)
        $text = $value
        if ($value -match "[\s'`"``$;,(){}@|&<>#]") { $text = "'" + ($value -replace "'", "''") + "'" }
        # inserted text, text in the menu, kind, tooltip; each object is returned as output
        [System.Management.Automation.CompletionResult]::new($text, $value, 'ParameterValue', $help)
    }
}
