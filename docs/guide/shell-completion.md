# Shell completion

maxicom can complete its options and their values when you press Tab in
bash, zsh, fish and PowerShell. It completes your own names too: your saved
links, the serial ports, your profiles and plots, and, for `maxicom cli`,
the maxicoms running and the commands of the command line.

## How it works

Each shell gets a short script from maxicom. On each Tab, the script runs
`maxicom` with the words typed so far, and maxicom answers with what fits.
So the script never goes out of date: a new option, a new saved link or a
new profile completes as soon as it exists.

Answering a Tab only reads your config files. It never writes one, never
opens a port and never sends anything to a running maxicom; it takes a few
milliseconds.

`maxicom --completions <shell>` prints the script, for `bash`, `zsh`,
`fish` or `powershell`.

## Setting it up

Pick the line for your shell. Open a new shell afterwards.

| Shell | Set up |
| --- | --- |
| bash | `eval "$(maxicom --completions bash)"` at the end of `~/.bashrc` |
| zsh | `eval "$(maxicom --completions zsh)"` in `~/.zshrc`, after `compinit` |
| fish | `maxicom --completions fish > ~/.config/fish/completions/maxicom.fish`, once |
| PowerShell | `maxicom --completions powershell \| Out-String \| Invoke-Expression` in your `$PROFILE` |

### Saving the script as a file

Instead of the `eval` line, you can save the script where the shell finds
it by itself. It then loads on the first Tab. Write the file again after
upgrading maxicom, in case the script changed.

| Shell | File |
| --- | --- |
| bash | `~/.local/share/bash-completion/completions/maxicom`, with the bash-completion package installed |
| zsh | `_maxicom` in a folder of `$fpath`, for example `~/.zfunc/_maxicom` with `fpath=(~/.zfunc $fpath)` before `compinit` |
| fish | `~/.config/fish/completions/maxicom.fish` |

```bash
maxicom --completions bash > ~/.local/share/bash-completion/completions/maxicom
```

### From an archive or a package

The [release archives](install.md#prebuilt-binaries) hold the scripts in a
`completions` folder: `maxicom.bash`, `_maxicom` for zsh, `maxicom.fish` and
`maxicom.ps1`. A package manager that installs them puts them where the
shell finds them, so there is nothing to set up.

## What it completes

| You type | Tab offers |
| --- | --- |
| `maxicom ` | Your saved links for this machine, then the serial ports |
| `maxicom -` | The options, with what each does in zsh, fish and PowerShell |
| `maxicom --parity ` | The values the option takes: `none`, `even`, `odd` |
| `maxicom -b 11` | The usual baud rates starting so: `115200` |
| `maxicom -D ` | The serial ports, never a saved link's name |
| `maxicom --profile ` | The profiles that are on |
| `maxicom --plot ` | The plots of `plots.toml` |
| `maxicom -C ` | The files, as the shell lists them |
| `maxicom cli ` | `list`, `events`, `rx`, then the commands and your `[commands]` |
| `maxicom cli --instance ` | The maxicoms running, by pid, with their link |
| `maxicom cli baud ` | What the [command line](command-line.md) offers there |
| `maxicom cli events ` | The event kinds not given yet |

A value glued to its option completes too: `--parity=e` becomes
`--parity=even`, and `--config=` lists the files.

When you give `--config` on the line, the links and profiles offered are the
ones of that settings file.

## Differences between the shells

bash shows the values only. zsh, fish and PowerShell also show what each
option does, and say whether a name is a saved link or a serial port.

When maxicom has nothing to offer, bash and PowerShell list the files
instead, as they do for any program. zsh and fish offer nothing.

The PowerShell script needs PowerShell 5.1 or newer.
