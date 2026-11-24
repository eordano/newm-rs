"""Qt 6 probes of the protocol cases; each appends JSON lines to OUT.

    qtx.py ROLE APP_ID OUT [key=value ...]

Roles: dnd-src (drag source offering `text`), dnd-dst (logs text dropped on it), fc (Enter opens
QFileDialog: the KDE dialog in-process, or the portal's with QT_QPA_PLATFORMTHEME=xdgdesktopportal).
"""

import json
import sys
import time

from PyQt6.QtCore import QMimeData, Qt
from PyQt6.QtGui import QColor, QDrag, QPalette
from PyQt6.QtWidgets import QApplication, QFileDialog, QLineEdit, QVBoxLayout, QWidget

ROLE, APP_ID, OUT = sys.argv[1], sys.argv[2], sys.argv[3]
OPT = dict(a.split("=", 1) for a in sys.argv[4:])
out = open(OUT, "a", buffering=1)
T0 = time.monotonic()


def log(**d):
    d["at"] = round(time.monotonic() - T0, 3)
    out.write(json.dumps(d) + "\n")


app = QApplication([APP_ID])
app.setDesktopFileName(APP_ID)


class Src(QWidget):
    def mousePressEvent(self, ev):
        log(press=True)
        drag = QDrag(self)
        mime = QMimeData()
        mime.setText(OPT.get("text", f"dnd-{APP_ID}"))
        drag.setMimeData(mime)
        log(drag="exec")
        res = drag.exec(Qt.DropAction.CopyAction)
        log(drag="end", result=int(res.value) if hasattr(res, "value") else int(res))


class Dst(QWidget):
    def __init__(self):
        super().__init__()
        self.setAcceptDrops(True)

    def dragEnterEvent(self, ev):
        log(drop="enter", formats=list(ev.mimeData().formats()))
        if ev.mimeData().hasText():
            ev.acceptProposedAction()

    def dragMoveEvent(self, ev):
        ev.acceptProposedAction()

    def dropEvent(self, ev):
        log(dropped=ev.mimeData().text())
        ev.acceptProposedAction()


class Fc(QWidget):
    def __init__(self):
        super().__init__()
        self.edit = QLineEdit()
        lay = QVBoxLayout(self)
        lay.setContentsMargins(40, 40, 40, 40)
        lay.addWidget(self.edit)
        self.edit.returnPressed.connect(self.go)

    def go(self):
        log(asked=True)
        dlg = QFileDialog(self, "probe-file-dialog")
        dlg.setModal(True)
        dlg.finished.connect(lambda r: log(response=r))
        dlg.open()
        self._dlg = dlg


win = {"dnd-src": Src, "dnd-dst": Dst, "fc": Fc}[ROLE]()
win.setWindowTitle(APP_ID)
pal = win.palette()
pal.setColor(QPalette.ColorRole.Window, QColor(OPT.get("color", "#30c030")))
win.setPalette(pal)
win.setAutoFillBackground(True)
win.resize(500, 400)
win.show()
sys.exit(app.exec())
