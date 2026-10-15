import QtQuick
import QtQuick.Controls as QQC2

Item {
    property string cfg_Images

    implicitHeight: label.implicitHeight

    QQC2.Label {
        id: label
        width: parent.width
        wrapMode: Text.WordWrap
        text: "Images for each monitor are managed by the muralist app."
    }
}
