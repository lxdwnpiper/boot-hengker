#!/usr/bin/env nu

def user-unit-dir []: nothing -> path {
    $env.XDG_RUNTIME_DIR | path join systemd user
}

def unit-names [units: path]: nothing -> list<string> {
    ls $units | get name | path basename
}

def install-units [units: path] {
    let target_dir = user-unit-dir
    mkdir $target_dir
    unit-names $units | each {|name| ^ln -sfn ($units | path join $name) ($target_dir | path join $name) } | ignore
    ^systemctl --user daemon-reload
}

def "main start" [--units: path, --target: string] {
    install-units $units
    ^systemctl --user stop ...(unit-names $units)
    ^systemctl --user start --no-block $target
    print $"started ($target); watch it with: boot-tree --user --target ($target) --hold 3s"
}

def "main remove" [--units: path, --target: string] {
    ^systemctl --user stop ...(unit-names $units)
    unit-names $units | each {|name| rm -f (user-unit-dir | path join $name) } | ignore
    ^systemctl --user daemon-reload
}

def main [--units: path, --target: string] {
    print "usage: boot-tree-demo-units (start | remove)"
}
