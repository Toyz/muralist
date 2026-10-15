import QtQuick
import QtQuick.Window
import org.kde.plasma.plasmoid

WallpaperItem {
    id: root

    readonly property var images: {
        try {
            return JSON.parse(root.configuration.Images || "{}");
        } catch (e) {
            return {};
        }
    }

    readonly property string imagePath: {
        const byName = images[Screen.name];
        if (byName) return byName;
        const byPos = images[Screen.virtualX + "," + Screen.virtualY];
        if (byPos) return byPos;
        return images["*"] || "";
    }

    Rectangle {
        anchors.fill: parent
        color: "black"
    }

    Image {
        anchors.fill: parent
        source: root.imagePath ? "file://" + root.imagePath : ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        cache: false
        smooth: true
        sourceSize.width: Math.round(root.width * Screen.devicePixelRatio)
        sourceSize.height: Math.round(root.height * Screen.devicePixelRatio)
    }
}
