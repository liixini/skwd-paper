#include "skwdvideoitem.h"

#include <QImage>
#include <QDebug>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMutexLocker>
#include <QOpenGLContext>
#include <QQuickWindow>
#include <QSGSimpleTextureNode>
#include <QSGTexture>
#include <QtGui/qopenglcontext_platform.h>
#include <QtQuick/qsgtexture_platform.h>
#include <QSocketNotifier>
#include <QTimer>
#include <QtEndian>
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GL/gl.h>
#include <drm_fourcc.h>
#include <array>
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <sys/socket.h>
#include <unistd.h>

namespace {
constexpr int SlotCount = 3;

quint32 read32(const uchar *data)
{
    return qFromLittleEndian<quint32>(data);
}

quint64 read64(const uchar *data)
{
    return qFromLittleEndian<quint64>(data);
}

class SkwdFrameNode final : public QSGSimpleTextureNode {
public:
    struct Texture {
        EGLImageKHR image = EGL_NO_IMAGE_KHR;
        GLuint name = 0;
        GLuint memory = 0;
        GLuint semaphore = 0;
        QSGTexture *texture = nullptr;
    };

    ~SkwdFrameNode() override
    {
        const bool hasContext = QOpenGLContext::currentContext() != nullptr;
        for (auto &slot : textures) {
            delete slot.texture;
            if (hasContext && slot.name != 0) {
                glDeleteTextures(1, &slot.name);
            }
            if (hasContext && slot.memory != 0 && deleteMemoryObjects) {
                deleteMemoryObjects(1, &slot.memory);
            }
            if (hasContext && slot.semaphore != 0 && deleteSemaphores) {
                deleteSemaphores(1, &slot.semaphore);
            }
            if (hasContext && slot.image != EGL_NO_IMAGE_KHR && destroyImage) {
                destroyImage(display, slot.image);
            }
        }
    }

    bool import(int index, SkwdVideoItem::DmabufSlot &slot, QQuickWindow *window)
    {
        if (textures[index].texture) {
            return true;
        }
        auto *context = QOpenGLContext::currentContext();
        if (!context) {
            return false;
        }
        auto *native = context->nativeInterface<QNativeInterface::QEGLContext>();
        if (!native) {
            return false;
        }
        display = native->display();
        createImage = reinterpret_cast<PFNEGLCREATEIMAGEKHRPROC>(eglGetProcAddress("eglCreateImageKHR"));
        destroyImage = reinterpret_cast<PFNEGLDESTROYIMAGEKHRPROC>(eglGetProcAddress("eglDestroyImageKHR"));
        imageTarget = reinterpret_cast<PFNGLEGLIMAGETARGETTEXTURE2DOESPROC>(eglGetProcAddress("glEGLImageTargetTexture2DOES"));
        if (!createImage || !destroyImage || !imageTarget) {
            return false;
        }
        const EGLint attributes[] = {
            EGL_WIDTH, EGLint(slot.width),
            EGL_HEIGHT, EGLint(slot.height),
            EGL_LINUX_DRM_FOURCC_EXT, EGLint(DRM_FORMAT_XRGB8888),
            EGL_DMA_BUF_PLANE0_FD_EXT, slot.fd,
            EGL_DMA_BUF_PLANE0_OFFSET_EXT, EGLint(slot.offset),
            EGL_DMA_BUF_PLANE0_PITCH_EXT, EGLint(slot.stride),
            EGL_NONE,
        };
        auto &target = textures[index];
        if (slot.opaque) {
            createMemoryObjects = reinterpret_cast<PFNGLCREATEMEMORYOBJECTSEXTPROC>(
                eglGetProcAddress("glCreateMemoryObjectsEXT"));
            deleteMemoryObjects = reinterpret_cast<PFNGLDELETEMEMORYOBJECTSEXTPROC>(
                eglGetProcAddress("glDeleteMemoryObjectsEXT"));
            memoryParameter = reinterpret_cast<PFNGLMEMORYOBJECTPARAMETERIVEXTPROC>(
                eglGetProcAddress("glMemoryObjectParameterivEXT"));
            importMemoryFd = reinterpret_cast<PFNGLIMPORTMEMORYFDEXTPROC>(
                eglGetProcAddress("glImportMemoryFdEXT"));
            textureStorageMemory = reinterpret_cast<PFNGLTEXTURESTORAGEMEM2DEXTPROC>(
                eglGetProcAddress("glTextureStorageMem2DEXT"));
            genSemaphores = reinterpret_cast<PFNGLGENSEMAPHORESEXTPROC>(
                eglGetProcAddress("glGenSemaphoresEXT"));
            deleteSemaphores = reinterpret_cast<PFNGLDELETESEMAPHORESEXTPROC>(
                eglGetProcAddress("glDeleteSemaphoresEXT"));
            importSemaphoreFd = reinterpret_cast<PFNGLIMPORTSEMAPHOREFDEXTPROC>(
                eglGetProcAddress("glImportSemaphoreFdEXT"));
            waitSemaphore = reinterpret_cast<PFNGLWAITSEMAPHOREEXTPROC>(
                eglGetProcAddress("glWaitSemaphoreEXT"));
            if (!createMemoryObjects || !deleteMemoryObjects || !memoryParameter
                || !importMemoryFd || !textureStorageMemory || !genSemaphores
                || !deleteSemaphores || !importSemaphoreFd || !waitSemaphore
                || slot.semaphoreFd < 0) {
                return false;
            }
            createMemoryObjects(1, &target.memory);
            const GLint dedicated = GL_TRUE;
            memoryParameter(target.memory, GL_DEDICATED_MEMORY_OBJECT_EXT, &dedicated);
            importMemoryFd(target.memory, slot.modifier, GL_HANDLE_TYPE_OPAQUE_FD_EXT, slot.fd);
            slot.fd = -1;
            glGenTextures(1, &target.name);
            glBindTexture(GL_TEXTURE_2D, target.name);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_R, GL_BLUE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_G, GL_GREEN);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_B, GL_RED);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_A, GL_ALPHA);
            glBindTexture(GL_TEXTURE_2D, 0);
            textureStorageMemory(target.name, 1, GL_RGBA8, slot.width, slot.height, target.memory, 0);
            const GLenum error = glGetError();
            qInfo() << "skwd-wallpaper: GL memory-object import" << error;
            if (error != GL_NO_ERROR) {
                return false;
            }
            target.texture = QNativeInterface::QSGOpenGLTexture::fromNative(
                target.name, window, QSize(int(slot.width), int(slot.height)));
            if (!target.texture) {
                return false;
            }
            genSemaphores(1, &target.semaphore);
            importSemaphoreFd(target.semaphore, GL_HANDLE_TYPE_OPAQUE_FD_EXT, slot.semaphoreFd);
            slot.semaphoreFd = -1;
            target.texture->setFiltering(QSGTexture::Linear);
            qInfo() << "skwd-wallpaper: imported Vulkan memory slot" << index;
            return true;
        }
        target.image = createImage(display, EGL_NO_CONTEXT, EGL_LINUX_DMA_BUF_EXT, nullptr, attributes);
        if (target.image == EGL_NO_IMAGE_KHR) {
            qWarning() << "skwd-wallpaper: EGL DMA-BUF import failed" << eglGetError();
            return false;
        }
        glGenTextures(1, &target.name);
        glBindTexture(GL_TEXTURE_2D, target.name);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
        imageTarget(GL_TEXTURE_2D, target.image);
        qInfo() << "skwd-wallpaper: GL" << reinterpret_cast<const char *>(glGetString(GL_VENDOR))
                << reinterpret_cast<const char *>(glGetString(GL_RENDERER)) << glGetError();
        glBindTexture(GL_TEXTURE_2D, 0);
        target.texture = QNativeInterface::QSGOpenGLTexture::fromNative(
            target.name, window, QSize(int(slot.width), int(slot.height)));
        if (!target.texture) {
            return false;
        }
        target.texture->setFiltering(QSGTexture::Linear);
        ::close(slot.fd);
        slot.fd = -1;
        qInfo() << "skwd-wallpaper: imported DMA-BUF slot" << index;
        return true;
    }

    bool wait(int index)
    {
        auto &target = textures[index];
        if (!waitSemaphore || target.semaphore == 0 || target.name == 0) {
            return false;
        }
        const GLenum layout = GL_LAYOUT_GENERAL_EXT;
        waitSemaphore(target.semaphore, 0, nullptr, 1, &target.name, &layout);
        return glGetError() == GL_NO_ERROR;
    }

    std::array<Texture, SlotCount> textures;
    EGLDisplay display = EGL_NO_DISPLAY;
    PFNEGLCREATEIMAGEKHRPROC createImage = nullptr;
    PFNEGLDESTROYIMAGEKHRPROC destroyImage = nullptr;
    PFNGLEGLIMAGETARGETTEXTURE2DOESPROC imageTarget = nullptr;
    PFNGLCREATEMEMORYOBJECTSEXTPROC createMemoryObjects = nullptr;
    PFNGLDELETEMEMORYOBJECTSEXTPROC deleteMemoryObjects = nullptr;
    PFNGLMEMORYOBJECTPARAMETERIVEXTPROC memoryParameter = nullptr;
    PFNGLIMPORTMEMORYFDEXTPROC importMemoryFd = nullptr;
    PFNGLTEXTURESTORAGEMEM2DEXTPROC textureStorageMemory = nullptr;
    PFNGLGENSEMAPHORESEXTPROC genSemaphores = nullptr;
    PFNGLDELETESEMAPHORESEXTPROC deleteSemaphores = nullptr;
    PFNGLIMPORTSEMAPHOREFDEXTPROC importSemaphoreFd = nullptr;
    PFNGLWAITSEMAPHOREEXTPROC waitSemaphore = nullptr;
    quint64 generation = 0;
    quint64 streamGeneration = 0;
    int currentSlot = -1;
};
}

SkwdVideoItem::SkwdVideoItem(QQuickItem *parent)
    : QQuickItem(parent)
{
    setFlag(ItemHasContents, true);
    m_process.setProcessChannelMode(QProcess::SeparateChannels);
    connect(&m_process, &QProcess::readyReadStandardOutput, this, &SkwdVideoItem::consume);
    connect(&m_process, &QProcess::readyReadStandardError, this, [this] {
        m_process.readAllStandardError();
    });
    connect(&m_process, &QProcess::finished, this, [this] {
        if (m_stopping || m_destroying || m_assignment.isEmpty()) {
            return;
        }
        QTimer::singleShot(1000, this, [this] {
            if (!m_destroying && m_process.state() == QProcess::NotRunning && !m_assignment.isEmpty()) {
                scheduleRestart();
            }
        });
    });
}

SkwdVideoItem::~SkwdVideoItem()
{
    m_destroying = true;
    m_stopping = true;
    closeDmabuf();
    m_process.terminate();
    if (!m_process.waitForFinished(500)) {
        m_process.kill();
        m_process.waitForFinished(500);
    }
}

QString SkwdVideoItem::assignment() const
{
    return m_assignment;
}

void SkwdVideoItem::setAssignment(const QString &value)
{
    if (m_assignment == value) {
        return;
    }
    m_assignment = value;
    emit assignmentChanged();
    if (isComponentComplete()) {
        scheduleRestart();
    }
}

QString SkwdVideoItem::paper() const
{
    return m_paper;
}

void SkwdVideoItem::setPaper(const QString &value)
{
    if (m_paper == value) {
        return;
    }
    m_paper = value;
    emit paperChanged();
    if (isComponentComplete()) {
        scheduleRestart();
    }
}

int SkwdVideoItem::streamWidth() const
{
    return m_streamWidth;
}

void SkwdVideoItem::setStreamWidth(int value)
{
    value = qBound(16, value, 7680);
    if (m_streamWidth == value) {
        return;
    }
    m_streamWidth = value;
    emit streamSizeChanged();
    if (isComponentComplete()) {
        scheduleRestart();
    }
}

int SkwdVideoItem::streamHeight() const
{
    return m_streamHeight;
}

void SkwdVideoItem::setStreamHeight(int value)
{
    value = qBound(16, value, 4320);
    if (m_streamHeight == value) {
        return;
    }
    m_streamHeight = value;
    emit streamSizeChanged();
    if (isComponentComplete()) {
        scheduleRestart();
    }
}

int SkwdVideoItem::streamFps() const
{
    return m_streamFps;
}

void SkwdVideoItem::setStreamFps(int value)
{
    value = qBound(1, value, 240);
    if (m_streamFps == value) {
        return;
    }
    m_streamFps = value;
    emit streamFpsChanged();
    if (isComponentComplete()) {
        scheduleRestart();
    }
}

bool SkwdVideoItem::paused() const
{
    return m_paused;
}

void SkwdVideoItem::setPaused(bool value)
{
    if (m_paused == value) {
        return;
    }
    m_paused = value;
    emit pausedChanged();
    if (isComponentComplete()) {
        sendPause();
    }
}

void SkwdVideoItem::componentComplete()
{
    QQuickItem::componentComplete();
    restart();
}

void SkwdVideoItem::scheduleRestart()
{
    m_restartRequired = true;
    if (m_restartScheduled) {
        return;
    }
    m_restartScheduled = true;
    QTimer::singleShot(0, this, [this] {
        m_restartScheduled = false;
        if (m_restartRequired) {
            m_restartRequired = false;
            restart();
        }
    });
}

void SkwdVideoItem::sendControl(const QByteArray &line)
{
    if (m_process.state() == QProcess::Running && m_process.write(line) < 0) {
        scheduleRestart();
    }
}

void SkwdVideoItem::sendPause()
{
    QJsonObject command;
    command.insert(QStringLiteral("to"), QString());
    command.insert(QStringLiteral("pause"), m_paused);
    sendControl(QJsonDocument(command).toJson(QJsonDocument::Compact) + '\n');
}

void SkwdVideoItem::closeDmabuf()
{
    delete m_socketNotifier;
    m_socketNotifier = nullptr;
    if (m_socket >= 0) {
        ::close(m_socket);
        m_socket = -1;
    }
    QMutexLocker lock(&m_frameMutex);
    for (auto &slot : m_slots) {
        if (slot.fd >= 0) {
            ::close(slot.fd);
        }
        if (slot.semaphoreFd >= 0) {
            ::close(slot.semaphoreFd);
        }
        slot = {};
    }
    m_pendingSlot = -1;
}

void SkwdVideoItem::restart()
{
    ++m_streamGeneration;
    m_stopping = true;
    if (m_process.state() != QProcess::NotRunning) {
        m_process.kill();
        m_process.waitForFinished(500);
    }
    closeDmabuf();
    m_stopping = false;
    m_buffer.clear();
    m_headerWidth = 0;
    m_headerHeight = 0;
    {
        QMutexLocker lock(&m_frameMutex);
        m_frame.clear();
        m_frameWidth = 0;
        m_frameHeight = 0;
    }
    m_frameBytes = 0;
    m_ready = false;
    if (m_assignment.isEmpty() || m_paper.isEmpty()) {
        update();
        return;
    }
    int sockets[2];
    if (::socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0, sockets) != 0) {
        return;
    }
    m_socket = sockets[0];
    const int flags = ::fcntl(m_socket, F_GETFL, 0);
    ::fcntl(m_socket, F_SETFL, flags | O_NONBLOCK);
    const int childSocket = sockets[1];
    m_process.setChildProcessModifier([childSocket] {
        ::dup2(childSocket, 3);
        if (childSocket != 3) {
            ::close(childSocket);
        }
    });
    m_socketNotifier = new QSocketNotifier(m_socket, QSocketNotifier::Read, this);
    connect(m_socketNotifier, &QSocketNotifier::activated, this, &SkwdVideoItem::consumeDmabuf);
    QStringList arguments {QStringLiteral("present-plasma"),
            QStringLiteral("--assignment"), m_assignment,
            QStringLiteral("--stream-size"),
            QStringLiteral("%1x%2").arg(m_streamWidth).arg(m_streamHeight),
            QStringLiteral("--stream-fps"), QString::number(m_streamFps),
            QStringLiteral("--stream-fd"), QStringLiteral("3")};
    if (m_paused) {
        arguments.append(QStringLiteral("--paused"));
    }
    m_process.start(m_paper, arguments);
    ::close(childSocket);
}

void SkwdVideoItem::consumeDmabuf()
{
    for (;;) {
        std::array<uchar, 32> bytes {};
        std::array<uchar, CMSG_SPACE(sizeof(int))> control {};
        iovec iov {bytes.data(), bytes.size()};
        msghdr message {};
        message.msg_iov = &iov;
        message.msg_iovlen = 1;
        message.msg_control = control.data();
        message.msg_controllen = control.size();
        const ssize_t length = ::recvmsg(m_socket, &message, MSG_DONTWAIT);
        if (length < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
            return;
        }
        if (length <= 0) {
            return;
        }
        if (length < 6 || std::memcmp(bytes.data(), "SKDG", 4) != 0) {
            continue;
        }
        const int slotIndex = bytes[5];
        if (slotIndex < 0 || slotIndex >= SlotCount) {
            continue;
        }
        if ((bytes[4] == 1 || bytes[4] == 4 || bytes[4] == 5) && length == 32) {
            int receivedFd = -1;
            for (cmsghdr *header = CMSG_FIRSTHDR(&message); header; header = CMSG_NXTHDR(&message, header)) {
                if (header->cmsg_level == SOL_SOCKET && header->cmsg_type == SCM_RIGHTS) {
                    std::memcpy(&receivedFd, CMSG_DATA(header), sizeof(receivedFd));
                    break;
                }
            }
            QMutexLocker lock(&m_frameMutex);
            auto &slot = m_slots[slotIndex];
            if (bytes[4] == 5) {
                slot.semaphoreFd = receivedFd;
                continue;
            }
            slot.fd = receivedFd;
            slot.width = read32(bytes.data() + 8);
            slot.height = read32(bytes.data() + 12);
            slot.stride = read32(bytes.data() + 16);
            slot.offset = read32(bytes.data() + 20);
            slot.modifier = read64(bytes.data() + 24);
            slot.opaque = bytes[4] == 4;
        } else if (bytes[4] == 2) {
            int skipped = -1;
            {
                QMutexLocker lock(&m_frameMutex);
                skipped = m_pendingSlot;
                m_pendingSlot = slotIndex;
                ++m_generation;
            }
            if (skipped >= 0) {
                acknowledge(skipped);
            }
            update();
        }
    }
}

void SkwdVideoItem::acknowledge(int slot)
{
    if (m_socket < 0 || slot < 0) {
        return;
    }
    std::array<uchar, 32> bytes {};
    std::memcpy(bytes.data(), "SKDG", 4);
    bytes[4] = 3;
    bytes[5] = uchar(slot);
    ::send(m_socket, bytes.data(), bytes.size(), MSG_NOSIGNAL);
}

void SkwdVideoItem::consume()
{
    m_buffer.append(m_process.readAllStandardOutput());
    if (!m_ready) {
        if (m_buffer.size() < 12) {
            return;
        }
        if (m_buffer.first(4) != QByteArrayLiteral("SKWP")) {
            m_process.kill();
            return;
        }
        const auto *header = reinterpret_cast<const uchar *>(m_buffer.constData() + 4);
        const quint32 width = qFromLittleEndian<quint32>(header);
        const quint32 height = qFromLittleEndian<quint32>(header + 4);
        if (width == 0 || height == 0 || width > 7680 || height > 4320) {
            m_process.kill();
            return;
        }
        m_frameBytes = qsizetype(width) * qsizetype(height) * 4;
        m_headerWidth = int(width);
        m_headerHeight = int(height);
        m_buffer.remove(0, 12);
        m_ready = true;
    }
    while (m_ready && m_buffer.size() >= m_frameBytes) {
        QByteArray frame = m_buffer.first(m_frameBytes);
        m_buffer.remove(0, m_frameBytes);
        {
            QMutexLocker lock(&m_frameMutex);
            m_frame = std::move(frame);
            m_frameWidth = m_headerWidth;
            m_frameHeight = m_headerHeight;
            ++m_generation;
        }
        update();
    }
}

QSGNode *SkwdVideoItem::updatePaintNode(QSGNode *oldNode, UpdatePaintNodeData *)
{
    auto *node = static_cast<SkwdFrameNode *>(oldNode);
    int pending = -1;
    quint64 generation;
    bool hasCpuFrame;
    {
        QMutexLocker lock(&m_frameMutex);
        generation = m_generation;
        pending = m_pendingSlot;
        m_pendingSlot = -1;
        hasCpuFrame = !m_frame.isEmpty();
    }
    if (node && node->streamGeneration != m_streamGeneration && (pending >= 0 || hasCpuFrame)) {
        delete node;
        node = nullptr;
    }
    if (!node && pending < 0 && !hasCpuFrame) {
        return nullptr;
    }
    if (!node) {
        node = new SkwdFrameNode;
        node->streamGeneration = m_streamGeneration;
        node->setOwnsTexture(false);
        node->setFiltering(QSGTexture::Linear);
    }
    node->setRect(boundingRect());
    if (pending >= 0) {
        bool imported;
        {
            QMutexLocker lock(&m_frameMutex);
            imported = node->import(pending, m_slots[pending], window());
        }
        if (imported) {
            if (!node->wait(pending)) {
                acknowledge(pending);
                return node;
            }
            if (node->currentSlot >= 0 && node->currentSlot != pending) {
                glFinish();
                acknowledge(node->currentSlot);
            }
            node->setTexture(node->textures[pending].texture);
            node->setOwnsTexture(false);
            node->currentSlot = pending;
            node->generation = generation;
            return node;
        }
        acknowledge(pending);
    }
    QByteArray frame;
    int frameWidth;
    int frameHeight;
    {
        QMutexLocker lock(&m_frameMutex);
        if (generation == node->generation || m_frame.isEmpty()) {
            return node;
        }
        frame = m_frame;
        frameWidth = m_frameWidth;
        frameHeight = m_frameHeight;
    }
    if (frameWidth <= 0 || frameHeight <= 0
        || frame.size() != qsizetype(frameWidth) * qsizetype(frameHeight) * 4) {
        return node;
    }
    const QImage borrowed(reinterpret_cast<const uchar *>(frame.constData()), frameWidth,
        frameHeight, frameWidth * 4, QImage::Format_RGBA8888);
    // The scene graph may defer the texture upload until after this function returns. A QImage
    // wrapping QByteArray storage does not keep that storage alive, so detach into owned pixels.
    const QImage image = borrowed.copy();
    if (image.isNull()) {
        return node;
    }
    QSGTexture *texture = window()->createTextureFromImage(image, QQuickWindow::TextureIsOpaque);
    node->setTexture(texture);
    node->setOwnsTexture(true);
    node->generation = generation;
    return node;
}
