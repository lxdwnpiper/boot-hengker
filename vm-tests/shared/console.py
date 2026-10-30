import base64

BOOTVINE_TERMINAL = 12


def read_terminal():
    attributed, characters = machine.succeed(
        f"base64 -w0 /dev/vcsa{BOOTVINE_TERMINAL}; echo; "
        f"base64 -w0 /dev/vcsu{BOOTVINE_TERMINAL}"
    ).split()
    attributed = base64.b64decode(attributed)
    columns = attributed[1]
    attributes = attributed[5::2]
    text = base64.b64decode(characters).decode("utf-32-le")
    return columns, text, attributes


def as_rows(text, columns):
    return "\n".join(text[start : start + columns] for start in range(0, len(text), columns))


def screen():
    columns, text, _ = read_terminal()
    return as_rows(text, columns)


def colours(attribute, font_has_512_glyphs):
    if font_has_512_glyphs:
        return (attribute >> 1) & 0x0F, (attribute >> 5) & 0x07
    return attribute & 0x0F, (attribute >> 4) & 0x07


def assert_text_visible(font_has_512_glyphs=False):
    columns, text, attributes = read_terminal()

    def is_hidden(character, attribute):
        foreground, background = colours(attribute, font_has_512_glyphs)
        return character != " " and foreground == background

    hidden = "".join(
        "#" if is_hidden(character, attribute) else " "
        for character, attribute in zip(text, attributes)
    )
    assert "#" not in hidden, (
        f"text drawn in its background colour, marked #:\n{as_rows(hidden, columns)}\n{as_rows(text, columns)}"
    )


def wait_for_screen(*fragments):
    def shows_all(last_try):
        text = screen()
        if last_try:
            print(text)
            print(bootvine_journal())
        return all(fragment in text for fragment in fragments)

    retry(shows_all, timeout_seconds=60)


def foreground_terminal():
    return machine.succeed("cat /sys/class/tty/tty0/active").strip()


def assert_bootvine_in_front():
    active = foreground_terminal()
    assert active == f"tty{BOOTVINE_TERMINAL}", f"bootvine is not in front: {active}"


def bootvine_journal():
    return machine.succeed("journalctl -b -u bootvine -o cat")
