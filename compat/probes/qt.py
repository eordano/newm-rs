"""A Qt 6 probe window: green, one line edit. Enter writes what was typed, the platform, the
style, the platform theme and the device pixel ratio to OUT as JSON. Usage: qt.py APP_ID OUT"""

import json
import os
import sys

from PyQt6.QtGui import QColor, QPalette
from PyQt6.QtWidgets import QApplication, QLineEdit, QVBoxLayout, QWidget

APP_ID, OUT = sys.argv[1], sys.argv[2]
app = QApplication([APP_ID])
app.setDesktopFileName(APP_ID)
win = QWidget()
win.setWindowTitle(APP_ID)
pal = win.palette()
pal.setColor(QPalette.ColorRole.Window, QColor("#30c030"))
win.setPalette(pal)
win.setAutoFillBackground(True)
edit = QLineEdit()
lay = QVBoxLayout(win)
lay.setContentsMargins(40, 40, 40, 40)
lay.addWidget(edit)


def report():
    d = {
        "text": edit.text(),
        "platform": app.platformName(),
        "style": app.style().name(),
        "theme": os.environ.get("QT_QPA_PLATFORMTHEME", ""),
        "dpr": win.devicePixelRatioF(),
        "size": [win.width(), win.height()],
    }
    with open(OUT, "w") as f:
        json.dump(d, f)


edit.returnPressed.connect(report)
win.resize(600, 300)
win.show()
sys.exit(app.exec())
