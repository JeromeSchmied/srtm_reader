#!/usr/bin/env fish
for entry in (bat coords.csv)
    set lat "$(echo $entry | cut -d ',' -f 1)"
    set lon "$(echo $entry | cut -d ',' -f 2)"
    set ele "$(echo $entry | cut -d ',' -f 3)"
    set name "$(echo $entry | cut -d ',' -f 4)"

    echo "name: $name"
    echo "actual elevation: $ele"
    echo "found in srtm: "
    cargo r -q --example cli "'$lat,$lon'"
    echo -----------------
end
