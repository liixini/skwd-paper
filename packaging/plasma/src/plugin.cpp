#include "skwdvideoitem.h"

#include <QQmlExtensionPlugin>
#include <qqml.h>

class SkwdWallpaperPlugin final : public QQmlExtensionPlugin {
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)

public:
    void registerTypes(const char *uri) override
    {
        qmlRegisterType<SkwdVideoItem>(uri, 1, 0, "SkwdVideoItem");
    }
};

#include "plugin.moc"
