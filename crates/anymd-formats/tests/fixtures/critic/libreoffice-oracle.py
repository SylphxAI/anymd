#!/usr/bin/python3
"""Write LibreOffice Writer's accept-all and reject-all text of a document.

Usage: /usr/bin/python3 libreoffice-oracle.py FILE.docx|FILE.fodt PROFILE_DIR [--docx]

Needs LibreOffice Writer and its Python bridge (python3-uno), so it runs with
the system Python rather than uv. Output: FILE.accepted.txt and
FILE.rejected.txt next to FILE. With --docx, FILE.accepted.docx and
FILE.rejected.docx as well: the whole result saved as Word, tables and lists
included, which the plain text leaves out.

Run it on the .fodt when there is one: Writer re-reading its own .docx can
drop a tracked paragraph break that is in the file, so Accept All on the
source is the reliable result.
"""

import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import uno

# pyuno creates the com.sun.star modules at import time, so type checkers
# cannot see them; getClass returns the same classes.
PropertyValue = uno.getClass("com.sun.star.beans.PropertyValue")
NoConnectException = uno.getClass("com.sun.star.connection.NoConnectException")

COMMANDS = {
    "accepted": ".uno:AcceptAllTrackedChanges",
    "rejected": ".uno:RejectAllTrackedChanges",
}
# XCloseable.close(DeliverOwnership): the caller keeps no reference afterwards.
DELIVER_OWNERSHIP = True


def prop(name: str, *, value: object) -> Any:
    """Build a com.sun.star.beans.PropertyValue for a load or dispatch call."""
    property_value = PropertyValue()
    property_value.Name = name
    property_value.Value = value
    return property_value


def free_port() -> int:
    """Pick a port no other run is using, so two runs never share an office."""
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def connect(port: int, attempts: int = 60) -> Any:
    """Return the context of the office started on `port`, once it listens."""
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext(
        "com.sun.star.bridge.UnoUrlResolver", local
    )
    url = f"uno:socket,host=127.0.0.1,port={port};urp;StarOffice.ComponentContext"
    for _ in range(attempts):
        try:
            return resolver.resolve(url)
        except NoConnectException:
            time.sleep(1)
    message = f"LibreOffice did not listen on port {port}"
    raise SystemExit(message)


def main(document: Path, profile: Path, *, save_docx: bool = False) -> None:
    """Write the accepted and rejected text of `document` beside it."""
    if document.suffix not in {".docx", ".fodt"} or not document.is_file():
        message = f"not a .docx or .fodt file: {document}"
        raise SystemExit(message)
    port = free_port()
    soffice = shutil.which("soffice")
    if soffice is None:
        message = "soffice is not on PATH"
        raise SystemExit(message)
    office = subprocess.Popen(
        [
            soffice,
            f"-env:UserInstallation={profile.resolve().as_uri()}",
            "--headless",
            "--norestore",
            f"--accept=socket,host=127.0.0.1,port={port};urp;",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        context = connect(port)
        manager = context.ServiceManager
        desktop = manager.createInstanceWithContext(
            "com.sun.star.frame.Desktop", context
        )
        dispatcher = manager.createInstanceWithContext(
            "com.sun.star.frame.DispatchHelper", context
        )
        url = document.resolve().as_uri()
        for suffix, command in COMMANDS.items():
            doc = desktop.loadComponentFromURL(
                url, "_blank", 0, (prop("Hidden", value=True),)
            )
            dispatcher.executeDispatch(
                doc.getCurrentController().getFrame(), command, "", 0, ()
            )
            text = doc.getText().getString().replace("\r\n", "\n")
            document.with_suffix(f".{suffix}.txt").write_text(
                text + "\n", encoding="utf-8"
            )
            if save_docx:
                doc.storeToURL(
                    document.with_suffix(f".{suffix}.docx").resolve().as_uri(),
                    (prop("FilterName", value="MS Word 2007 XML"),),
                )
            doc.close(DELIVER_OWNERSHIP)
    finally:
        office.terminate()


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]), save_docx="--docx" in sys.argv[3:])
