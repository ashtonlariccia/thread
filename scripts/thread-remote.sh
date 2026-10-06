#!/bin/sh
# thread: open files and folders in the Thread window this terminal is in.
#
#   thread            the folder you are in
#   thread src        a folder, in the file tree
#   thread a.c b.c    files, each in a tab
#
# For a machine Thread connects to: put this on its PATH as `thread`
# (`~/.local/bin/thread`, made executable). It works by asking the terminal:
# each path is written to it in an escape sequence that Thread's terminal
# reads and no other does, so run anywhere else it does nothing at all. It
# does not reach through tmux or screen, which keep such things to themselves.

if ! [ -w /dev/tty ]; then
  echo "thread: this is not a terminal" >&2
  exit 1
fi
[ $# -eq 0 ] && set -- .

status=0
for target; do
  if [ -d "$target" ]; then
    kind=dir
    path=$(cd -- "$target" && pwd -P)
  elif [ -f "$target" ]; then
    kind=file
    path=$(cd -- "$(dirname -- "$target")" && pwd -P)/$(basename -- "$target")
  else
    echo "thread: there is no file or folder $target" >&2
    status=1
    continue
  fi
  # OSC 7717: what to open, as "dir" or "file", and where it is.
  printf '\033]7717;%s;%s\007' "$kind" "$path" >/dev/tty
done
exit $status
