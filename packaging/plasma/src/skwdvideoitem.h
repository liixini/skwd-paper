#pragma once

#include <QByteArray>
#include <QMutex>
#include <QProcess>
#include <QQuickItem>
#include <array>

class QSocketNotifier;

class SkwdVideoItem : public QQuickItem {
    Q_OBJECT
    Q_PROPERTY(QString assignment READ assignment WRITE setAssignment NOTIFY assignmentChanged)
    Q_PROPERTY(QString paper READ paper WRITE setPaper NOTIFY paperChanged)
    Q_PROPERTY(int streamWidth READ streamWidth WRITE setStreamWidth NOTIFY streamSizeChanged)
    Q_PROPERTY(int streamHeight READ streamHeight WRITE setStreamHeight NOTIFY streamSizeChanged)
    Q_PROPERTY(int streamFps READ streamFps WRITE setStreamFps NOTIFY streamFpsChanged)
    Q_PROPERTY(bool paused READ paused WRITE setPaused NOTIFY pausedChanged)

public:
    explicit SkwdVideoItem(QQuickItem *parent = nullptr);
    ~SkwdVideoItem() override;

    QString assignment() const;
    void setAssignment(const QString &value);
    QString paper() const;
    void setPaper(const QString &value);
    int streamWidth() const;
    void setStreamWidth(int value);
    int streamHeight() const;
    void setStreamHeight(int value);
    int streamFps() const;
    void setStreamFps(int value);
    bool paused() const;
    void setPaused(bool value);

signals:
    void assignmentChanged();
    void paperChanged();
    void streamSizeChanged();
    void streamFpsChanged();
    void pausedChanged();

protected:
    void componentComplete() override;
    QSGNode *updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *) override;

public:
    struct DmabufSlot {
        int fd = -1;
        int semaphoreFd = -1;
        quint32 width = 0;
        quint32 height = 0;
        quint32 stride = 0;
        quint32 offset = 0;
        quint64 modifier = 0;
        bool opaque = false;
    };

private:
    void scheduleRestart();
    void restart();
    void sendControl(const QByteArray &line);
    void sendPause();
    void consume();
    void consumeDmabuf();
    void closeDmabuf();
    void acknowledge(int slot);

    QString m_assignment;
    QString m_paper = QStringLiteral("/usr/bin/skwd-paper-v2");
    int m_streamWidth = 1280;
    int m_streamHeight = 720;
    int m_streamFps = 30;
    bool m_paused = false;
    bool m_restartScheduled = false;
    bool m_restartRequired = false;
    bool m_stopping = false;
    bool m_destroying = false;
    QProcess m_process;
    QByteArray m_buffer;
    QByteArray m_frame;
    int m_headerWidth = 0;
    int m_headerHeight = 0;
    int m_frameWidth = 0;
    int m_frameHeight = 0;
    QMutex m_frameMutex;
    std::array<DmabufSlot, 3> m_slots;
    QSocketNotifier *m_socketNotifier = nullptr;
    int m_socket = -1;
    int m_pendingSlot = -1;
    qsizetype m_frameBytes = 0;
    quint64 m_generation = 0;
    quint64 m_streamGeneration = 0;
    bool m_ready = false;
};
