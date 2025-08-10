#!/bin/sh
# rustbee system setup
# affix colons on either side of $PATH to simplify matching
case ":${PATH}:" in
    *:"$HOME/.rb":*)
        ;;
    *)
        # Prepending path in case a system-installed rustbee needs to be overridden
        export PATH="$HOME/.rb:$PATH"
        ;;
esac