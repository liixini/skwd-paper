import QtQuick
import QtQuick.Window
import org.kde.plasma.plasmoid
import org.skwd.wallpaper 1.0

WallpaperItem {
    id: root

    function parseAssignments(raw) {
        if (!raw)
            return ({})
        try {
            return JSON.parse(raw)
        } catch (error) {
            return ({})
        }
    }

    property var assignments: parseAssignments(configuration.Assignments || "")
    property var currentAssignment: assignments[Screen.name] || null

    SkwdVideoItem {
        anchors.fill: parent
        assignment: root.currentAssignment
            ? JSON.stringify(root.currentAssignment.assignment)
            : (configuration.Assignment || "")
        paper: root.currentAssignment
            ? root.currentAssignment.paper
            : (configuration.Paper || "skwd-paper-v2")
        streamWidth: root.width > 0
            ? Math.round(root.width * Math.max(1, Screen.devicePixelRatio))
            : (root.currentAssignment ? root.currentAssignment.width : (configuration.StreamWidth || 1920))
        streamHeight: root.height > 0
            ? Math.round(root.height * Math.max(1, Screen.devicePixelRatio))
            : (root.currentAssignment ? root.currentAssignment.height : (configuration.StreamHeight || 1080))
        streamFps: Screen.refreshRate > 0
            ? Math.min(root.currentAssignment ? root.currentAssignment.fps : (configuration.StreamFps || 30), Math.round(Screen.refreshRate))
            : (root.currentAssignment ? root.currentAssignment.fps : (configuration.StreamFps || 30))
        paused: root.currentAssignment ? root.currentAssignment.paused : (configuration.Paused || false)
    }

    Component.onCompleted: root.loading = false
}
