#!/bin/sh
# tide: open and close files and folders in the Tide window this
# terminal is in.
#
#   tide               open the folder you are in
#   tide src           a folder, in the file tree
#   tide a.c b.c       files, each in a tab
#   tide -c a.c src    close a file's tab, or a folder (--close)
#   tide -c            close the folder you are in
#
# For a machine Tide connects to: put this on its PATH as `tide`
# (`~/.local/bin/tide`, made executable). It works by asking the terminal:
# each path is written to it in an escape sequence that Tide's terminal
# reads and no other does, so run anywhere else it does nothing at all. It
# does not reach through tmux or screen, which keep such things to themselves.

usage() {
  echo "usage: tide [-c|--close] [file or folder ...]"
}

close=false
case "${1:-}" in
  -c | --close)
    close=true
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  -?*)
    echo "tide: there is no option $1" >&2
    usage >&2
    exit 2
    ;;
esac

if ! [ -w /dev/tty ]; then
  echo "tide: this is not a terminal" >&2
  exit 1
fi
[ $# -eq 0 ] && set -- .

status=0
for target; do
  if [ -d "$target" ]; then
    kind=dir
    path=$(cd -- "$target" && pwd -P)
  # Something being closed need not still be there: a file open in a tab
  # can have been deleted since. Its folder has to be, to say where it was.
  elif [ -f "$target" ] || { $close && [ -d "$(dirname -- "$target")" ]; }; then
    kind=file
    path=$(cd -- "$(dirname -- "$target")" && pwd -P)/$(basename -- "$target")
  else
    echo "tide: there is no file or folder $target" >&2
    status=1
    continue
  fi
  # Tide works out for itself whether what is closed is a tab or a folder.
  $close && kind=close
  # OSC 7717: what to do, as "dir", "file" or "close", and to what.
  printf '\033]7717;%s;%s\007' "$kind" "$path" >/dev/tty
done
exit $status
