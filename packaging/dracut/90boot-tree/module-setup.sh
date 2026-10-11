#!/usr/bin/bash

check() {
    require_binaries boot-tree || return 1
    return 255
}

depends() {
    echo systemd
}

install() {
    inst_binary boot-tree
    inst_simple "$systemdsystemunitdir/boot-tree.service"
    $SYSTEMCTL -q --root "$initdir" enable boot-tree.service
}
