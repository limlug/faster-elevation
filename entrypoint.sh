#!/bin/sh

# Check if the REGENERATE environment variable is set to "true"
if [ "$REGENERATE" = "true" ]; then
    # Execute /bin/server with --regenerate option, followed by normal execution
    /bin/faster-elevation --regenerate
    /bin/faster-elevation
else
    # Execute /bin/server normally
    /bin/faster-elevation
fi