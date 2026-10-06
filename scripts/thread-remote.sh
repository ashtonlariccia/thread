#!/bin/sh
# thread: open and close files and folders in the Thread window this
# terminal is in.
#
#   thread               open the folder you are in
#   thread src           a folder, in the file tree
#   thread a.c b.c       files, each in a tab
#   thread -c a.c src    close a file's tab, or a folder (--close)
#   thread -c            close the folder you are in
#
# For a machine Thread connects to: put this on its PATH as `thread`
# (`~/.local/bin/thread`, made executable). It works by asking the terminal:
# each path is written to it in an escape sequence that Thread's terminal
# reads and no other does, so run anywhere else it does nothing at all. It
# does not reach through tmux or screen, which keep such things to themselves.

usage() {
  echo "usage: thread [-c|--close] [file or folder ...]"
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
    echo "thread: there is no option $1" >&2
    usage >&2
    exit 2
    ;;
esac

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
  # Something being closed need not still be there: a file open in a tab
  # can have been deleted since. Its folder has to be, to say where it was.
  elif [ -f "$target" ] || { $close && [ -d "$(dirname -- "$target")" ]; }; then
    kind=file
    path=$(cd -- "$(dirname -- "$target")" && pwd -P)/$(basename -- "$target")
  else
    echo "thread: there is no file or folder $target" >&2
    status=1
    continue
  fi
  # Thread works out for itself whether what is closed is a tab or a folder.
  $close && kind=close
  # OSC 7717: what to do, as "dir", "file" or "close", and to what.
  printf '\033]7717;%s;%s\007' "$kind" "$path" >/dev/tty
done
exit $status
