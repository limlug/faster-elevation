#!/bin/sh

# Check if the REGENERATE environment variable is set to "true"
if [ "$REGENERATE" = "true" ]; then
    # Execute /bin/server with --regenerate option, followed by normal execution
    /bin/faster_elevation --regenerate
    /bin/faster_elevation
else
    # Execute /bin/server normally
    /bin/faster_elevation
fi