import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import {
  register,
  unregister,
  unregisterAll,
} from "@tauri-apps/plugin-global-shortcut";
import "./App.css";
import {
  createTranslator,
  languageOptions,
  normalizeLanguagePreference,
  resolveLanguage,
  type LanguagePreference,
} from "./i18n";

type ToolStatus = {
  name: string;
  found: boolean;
  path?: string;
  version?: string;
};

type DependencyStatus = {
  tools: ToolStatus[];
  missingTools: string[];
  isSatisfied: boolean;
};

type Preferences = {
  downloadFolder: string;
  language: LanguagePreference;
  localPlayPauseShortcut: LocalPlayPauseShortcut;
  globalActivationShortcutEnabled: boolean;
  globalActivationShortcut: string;
  updateCheckEnabled: boolean;
};

type LocalPlayPauseShortcut = "Space" | "Enter" | "P";

type DownloadItem = {
  id: string;
  sourceUrl: string;
  title: string;
  filePath: string;
  thumbnailPath?: string;
  createdAt: string;
};

type AppStateSnapshot = {
  preferences: Preferences;
  history: DownloadItem[];
  appDataDir: string;
};

type DownloadResult = {
  item: DownloadItem;
  history: DownloadItem[];
};

type TrimSelection = {
  start: number;
  end: number;
};

type TrimResult = {
  outputPath: string;
};

type TimelineFramesResult = {
  frames: string[];
};

type QueueItem = {
  id: string;
  url: string;
  status: "pending" | "running" | "success" | "error";
  progress: number | null;
  speed?: string;
  eta?: string;
  title?: string;
  error?: string;
};

type LoadState = "idle" | "loading" | "ready" | "error";

type ActiveDownload = {
  id: string;
  url: string;
  progress: number | null;
  speed?: string;
  eta?: string;
};

type DownloadProgressEvent = {
  requestId?: string;
  sourceUrl: string;
  status: string;
  percent?: number | null;
  speed?: string | null;
  eta?: string | null;
};

function App() {
  const [url, setUrl] = useState("");
  const [dependencyStatus, setDependencyStatus] =
    useState<DependencyStatus | null>(null);
  const [preferences, setPreferences] = useState<Preferences | null>(null);
  const [history, setHistory] = useState<DownloadItem[]>([]);
  const [loadState, setLoadState] = useState<LoadState>("idle");
  const [isDownloading, setIsDownloading] = useState(false);
  const [isChoosingFolder, setIsChoosingFolder] = useState(false);
  const [selectedItemId, setSelectedItemId] = useState<string | null>(null);
const [settingsOpen, setSettingsOpen] = useState(false);
  const [batchOpen, setBatchOpen] = useState(false);
  const [batchText, setBatchText] = useState("");
  const [downloadQueue, setDownloadQueue] = useState<QueueItem[]>([]);
  const [activeDownload, setActiveDownload] = useState<ActiveDownload | null>(null);
  const [statusText, setStatusText] = useState("");
  const activeLanguage = resolveLanguage(preferences?.language);
  const t = useMemo(() => createTranslator(activeLanguage), [activeLanguage]);

  useEffect(() => {
    void refreshApp();
  }, []);

  useEffect(() => {
    if (!statusText) {
      setStatusText(t("statusLoadingApp"));
    }
  }, [statusText, t]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<DownloadProgressEvent>("download-progress", (event) => {
      const progress = normalizeProgress(event.payload.percent);
      const nextProgress = progress ?? (event.payload.status === "finished" ? 100 : null);

      setActiveDownload((current) => {
        if (!current || !matchesDownloadProgress(current.id, current.url, event.payload)) {
          return current;
        }

        return {
          ...current,
          progress: nextProgress ?? current.progress,
          speed: event.payload.speed ?? undefined,
          eta: event.payload.eta ?? undefined,
        };
      });

      setDownloadQueue((queue) =>
        queue.map((item) => {
          if (!matchesDownloadProgress(item.id, item.url, event.payload)) {
            return item;
          }

          return {
            ...item,
            progress: nextProgress ?? item.progress,
            speed: event.payload.speed ?? undefined,
            eta: event.payload.eta ?? undefined,
          };
        }),
      );
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
      } else {
        unlisten = cleanup;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const canDownload = useMemo(() => {
    return (
      dependencyStatus?.isSatisfied === true &&
      preferences !== null &&
      looksLikeWebUrl(url.trim()) &&
      !isDownloading
    );
  }, [dependencyStatus, isDownloading, preferences, url]);

  const selectedItem = useMemo(() => {
    return history.find((item) => item.id === selectedItemId) ?? null;
  }, [history, selectedItemId]);

  const batchUrls = useMemo(() => parseUrls(batchText), [batchText]);

  const queueCompletedCount = useMemo(
    () => downloadQueue.filter((item) => item.status === "success" || item.status === "error").length,
    [downloadQueue],
  );

  const queueProgress = useMemo(() => {
    if (downloadQueue.length === 0) {
      return 0;
    }

    const totalProgress = downloadQueue.reduce((total, item) => {
      if (item.status === "success" || item.status === "error") {
        return total + 1;
      }

      return total + (normalizeProgress(item.progress) ?? 0) / 100;
    }, 0);

    return Math.round((totalProgress / downloadQueue.length) * 100);
  }, [downloadQueue]);

  const canDownloadBatch = useMemo(() => {
    return (
      dependencyStatus?.isSatisfied === true &&
      preferences !== null &&
      batchUrls.length > 0 &&
      !isDownloading
    );
  }, [batchUrls.length, dependencyStatus, isDownloading, preferences]);

  useEffect(() => {
    if (!preferences?.globalActivationShortcutEnabled) {
      void unregisterAll().catch(() => undefined);
      return;
    }

    let canceled = false;

    void unregisterAll()
      .then(() =>
        register(preferences.globalActivationShortcut, (event) => {
          if (event.state === "Pressed") {
            setStatusText(t("statusGlobalShortcutActivated"));
            window.focus();
          }
        }),
      )
      .then(() => {
        if (!canceled) {
          setStatusText(t("statusGlobalShortcutActive", { shortcut: preferences.globalActivationShortcut }));
        }
      })
      .catch((error) => {
        if (!canceled) {
          setStatusText(t("statusGlobalShortcutUnavailable", { error: formatError(error, t) }));
        }
      });

    return () => {
      canceled = true;
      void unregister(preferences.globalActivationShortcut).catch(() => undefined);
    };
  }, [preferences?.globalActivationShortcut, preferences?.globalActivationShortcutEnabled, t]);

  async function refreshApp() {
    setLoadState("loading");
    setStatusText(t("statusLoadingApp"));

    try {
      const [dependencies, appState] = await Promise.all([
        invoke<DependencyStatus>("check_dependencies"),
        invoke<AppStateSnapshot>("get_app_state"),
      ]);

      setDependencyStatus(dependencies);
      setPreferences(appState.preferences);
      setHistory(appState.history);
      setLoadState("ready");
      setStatusText(
        dependencies.isSatisfied
          ? t("statusReady")
          : t("statusMissing", { tools: dependencies.missingTools.join(", ") }),
      );
    } catch (error) {
      setLoadState("error");
      setStatusText(formatError(error, t));
    }
  }

  async function clearHistory() {
    try {
      const nextHistory = await invoke<DownloadItem[]>("clear_history");
      setHistory(nextHistory);
      setSelectedItemId(null);
      setStatusText(t("statusHistoryCleared"));
    } catch (error) {
      setStatusText(formatError(error, t));
    }
  }

  async function chooseDownloadFolder() {
    if (!preferences) {
      setStatusText(t("statusDownloadFolderNotReady"));
      return;
    }

    setIsChoosingFolder(true);
    setStatusText(t("statusChoosingDownloadFolder"));

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: preferences.downloadFolder,
        title: t("downloadFolder"),
      });

      if (typeof selected !== "string") {
        setStatusText(t("statusFolderSelectionCanceled"));
        return;
      }

      const nextPreferences = await saveDownloadFolder(selected);
      setStatusText(
        t("statusDownloadFolderSet", { folder: folderName(nextPreferences.downloadFolder) }),
      );
    } catch (error) {
      setStatusText(formatError(error, t));
    } finally {
      setIsChoosingFolder(false);
    }
  }

  async function useDefaultDownloadFolder() {
    setIsChoosingFolder(true);

    try {
      const nextPreferences = await saveDownloadFolder("");
      setStatusText(
        t("statusDownloadFolderSet", { folder: folderName(nextPreferences.downloadFolder) }),
      );
    } catch (error) {
      setStatusText(formatError(error, t));
    } finally {
      setIsChoosingFolder(false);
    }
  }

  async function saveDownloadFolder(downloadFolder: string) {
    const nextPreferences = await invoke<Preferences>("save_preferences", {
      preferences: { ...requirePreferences(preferences), downloadFolder },
    });
    setPreferences(nextPreferences);
    return nextPreferences;
  }

  async function saveSettings(nextPreferences: Preferences) {
    try {
      await unregisterAll().catch(() => undefined);

      if (nextPreferences.globalActivationShortcutEnabled) {
        await register(nextPreferences.globalActivationShortcut, () => undefined);
        await unregister(nextPreferences.globalActivationShortcut).catch(() => undefined);
      }

      const savedPreferences = await invoke<Preferences>("save_preferences", {
        preferences: nextPreferences,
      });
      setPreferences(savedPreferences);
      setStatusText(
        createTranslator(resolveLanguage(savedPreferences.language))("statusSettingsSaved"),
      );
      return savedPreferences;
    } catch (error) {
      setStatusText(t("statusGlobalShortcutUnavailable", { error: formatError(error, t) }));
      throw error;
    }
  }

  async function copyHistoryItem(item: DownloadItem) {
    try {
      await invoke("copy_file", { filePath: item.filePath });
      setStatusText(t("statusCopied", { title: displayTitle(item) }));
    } catch (error) {
      setStatusText(formatError(error, t));
    }
  }

  async function revealHistoryItem(item: DownloadItem) {
    try {
      await invoke("reveal_file", { filePath: item.filePath });
      setStatusText(t("statusOpenedExplorer"));
    } catch (error) {
      setStatusText(formatError(error, t));
    }
  }

  async function openHistorySource(item: DownloadItem) {
    try {
      await invoke("open_source_url", { sourceUrl: item.sourceUrl });
      setStatusText(t("statusOpenedSource"));
    } catch (error) {
      setStatusText(formatError(error, t));
    }
  }

  async function openAuthorGithub() {
    try {
      await invoke("open_source_url", { sourceUrl: "https://github.com/dogrunlabs" });
      setStatusText(t("statusOpenedSource"));
    } catch (error) {
      setStatusText(formatError(error, t));
    }
  }

  async function deleteHistoryItem(item: DownloadItem) {
    try {
      const nextHistory = await invoke<DownloadItem[]>("delete_history_item", {
        id: item.id,
      });
      setHistory(nextHistory);
      if (selectedItemId === item.id) {
        setSelectedItemId(null);
      }
      setStatusText(t("statusRemovedHistoryItem"));
    } catch (error) {
      setStatusText(formatError(error, t));
    }
  }

  async function submitDownload(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const sourceUrl = url.trim();

    if (!looksLikeWebUrl(sourceUrl)) {
      setStatusText(t("statusEnterValidUrl"));
      return;
    }

    await downloadSourceUrls([sourceUrl], {
      showQueue: false,
      onSuccess: () => setUrl(""),
    });
  }

  async function submitBatchDownload(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const sourceUrls = parseUrls(batchText);

    if (sourceUrls.length === 0) {
      setStatusText(t("statusPasteUrls"));
      return;
    }

    await downloadSourceUrls(sourceUrls, {
      showQueue: true,
      onSuccess: () => setBatchText(""),
    });
  }

  async function downloadSourceUrls(
    sourceUrls: string[],
    options: { showQueue: boolean; onSuccess: () => void },
  ) {
    if (!dependencyStatus?.isSatisfied) {
      setStatusText(t("statusInstallTools"));
      return;
    }

    if (!preferences) {
      setStatusText(t("statusDownloadFolderNotReady"));
      return;
    }

    const queueItems = sourceUrls.map((sourceUrl) => ({
      id: createDownloadRequestId(),
      url: sourceUrl,
      status: "pending" as const,
      progress: null,
    }));

    setIsDownloading(true);
    if (options.showQueue) {
      setDownloadQueue(queueItems);
      setActiveDownload(null);
    } else {
      setDownloadQueue([]);
      setActiveDownload({
        id: queueItems[0].id,
        url: queueItems[0].url,
        progress: null,
      });
    }
    setStatusText(
      sourceUrls.length === 1
        ? t("statusDownloading")
        : t("statusDownloadingOf", { current: 1, total: sourceUrls.length }),
    );

    const failures: QueueItem[] = [];
    let successCount = 0;
    let lastDownloadedTitle = "";

    try {
      for (let index = 0; index < queueItems.length; index += 1) {
        const queueItem = queueItems[index];
        if (options.showQueue) {
          setDownloadQueue((queue) =>
            queue.map((item, itemIndex) =>
              itemIndex === index ? { ...item, status: "running", progress: 0 } : item,
            ),
          );
        } else {
          setActiveDownload({
            id: queueItem.id,
            url: queueItem.url,
            progress: 0,
          });
        }
        setStatusText(
          queueItems.length === 1
            ? t("statusDownloading")
            : t("statusDownloadingOf", { current: index + 1, total: queueItems.length }),
        );

        try {
          const result = await invoke<DownloadResult>("download_media", {
            request: {
              sourceUrl: queueItem.url,
              requestId: queueItem.id,
            },
          });
          setHistory(result.history);
          successCount += 1;
          lastDownloadedTitle = displayTitle(result.item);
          if (options.showQueue) {
            setDownloadQueue((queue) =>
              queue.map((item, itemIndex) =>
                itemIndex === index
                  ? { ...item, status: "success", progress: 100, title: displayTitle(result.item) }
                  : item,
              ),
            );
          } else {
            setActiveDownload((current) =>
              current?.id === queueItem.id ? { ...current, progress: 100 } : current,
            );
          }
        } catch (error) {
          const message = formatError(error, t);
          const failedItem = {
            ...queueItem,
            status: "error" as const,
            progress: 100,
            error: message,
          };
          failures.push(failedItem);
          if (options.showQueue) {
            setDownloadQueue((queue) =>
              queue.map((item, itemIndex) => (itemIndex === index ? failedItem : item)),
            );
          } else {
            setActiveDownload(null);
          }
        }
      }

      if (failures.length === 0) {
        options.onSuccess();
          setStatusText(
            sourceUrls.length === 1
              ? t("statusDownloadedOne", { title: lastDownloadedTitle || "1 item" })
              : t("statusDownloadedMany", { count: sourceUrls.length }),
        );
      } else if (sourceUrls.length === 1) {
        setStatusText(failures[0]?.error || t("statusDownloadFailed"));
      } else {
        setStatusText(
          t("statusDownloadedPartial", {
            success: successCount,
            total: sourceUrls.length,
            failed: failures.length,
          }),
        );
      }
    } finally {
      setIsDownloading(false);
      if (!options.showQueue) {
        window.setTimeout(() => {
          setActiveDownload((current) => (current?.id === queueItems[0]?.id ? null : current));
        }, 900);
      }
    }
  }

  return (
    <main className="shell">
      <section className="workspace" aria-label={t("appName")}>
        <section className="top-workspace" aria-label={t("download")}>
          <div className="app-header">
            <div className="brand-block">
              <img className="brand-mark" src="/dogrun-logo.png" alt="" />
              <div className="app-title">
                <h1>{t("appName")}</h1>
                <p>{folderName(preferences?.downloadFolder) || t("appLoadingFolder")}</p>
              </div>
            </div>
            <div className="app-actions">
              <button
                className="outline-button"
                type="button"
                onClick={() => void openAuthorGithub()}
                title={t("openAuthorGithub")}
              >
                {t("github")}
              </button>
              <button
                className="secondary-button"
                type="button"
                onClick={() => void refreshApp()}
                disabled={loadState === "loading"}
              >
                {t("refresh")}
              </button>
              <button
                className="outline-button"
                type="button"
                onClick={() => setSettingsOpen((value) => !value)}
                disabled={!preferences}
              >
                {t("settings")}
              </button>
            </div>
          </div>

          <form className="download-bar" onSubmit={submitDownload}>
            <input
              className="url-input"
              value={url}
              onChange={(event) => setUrl(event.currentTarget.value)}
              placeholder={t("pasteVideoUrl")}
              spellCheck={false}
              autoFocus
            />
            <button className="primary-button" type="submit" disabled={!canDownload}>
              {isDownloading ? t("working") : t("download")}
            </button>
            <button
              className="outline-button batch-button"
              type="button"
              onClick={() => setBatchOpen(true)}
              disabled={!preferences}
            >
              {t("batch")}
            </button>
          </form>

          <div className="status-line" role="status">
            {statusText}
          </div>

          {activeDownload && (
            <section className="single-progress-panel" aria-label={t("progress")}>
              <div className="progress-header">
                <span>{t("progressDownload")}</span>
                <span>{formatProgress(activeDownload.progress, t)}</span>
              </div>
              <ProgressBar value={activeDownload.progress} />
              <div className="progress-detail">
                <span>{activeDownload.url}</span>
                <span>{formatTransferMeta(activeDownload, t)}</span>
              </div>
            </section>
          )}

          <div className="folder-strip">
            <div className="folder-summary">
              <span className="info-label">{t("downloadFolder")}</span>
              <span className="folder-path">
                {cleanPath(preferences?.downloadFolder) || t("loading")}
              </span>
            </div>
            <div className="folder-actions">
              <button
                className="secondary-button"
                type="button"
                onClick={() => void chooseDownloadFolder()}
                disabled={!preferences || isChoosingFolder || isDownloading}
              >
                {t("choose")}
              </button>
              <button
                className="outline-button"
                type="button"
                onClick={() => void useDefaultDownloadFolder()}
                disabled={!preferences || isChoosingFolder || isDownloading}
              >
                {t("useDownloads")}
              </button>
            </div>
          </div>
        </section>

        {batchOpen && (
          <div
            className="batch-modal-backdrop"
            role="presentation"
            onMouseDown={(event) => {
              if (event.target === event.currentTarget) {
                setBatchOpen(false);
              }
            }}
          >
            <section className="batch-modal" role="dialog" aria-modal="true" aria-label={t("batchTitle")}>
              <div className="batch-modal-header">
                <div>
                  <h2>{t("batchTitle")}</h2>
                  <p>{t("batchDescription")}</p>
                </div>
                <button className="outline-button" type="button" onClick={() => setBatchOpen(false)}>
                  {t("close")}
                </button>
              </div>

              <form className="batch-form" onSubmit={submitBatchDownload}>
                <textarea
                  className="batch-input"
                  value={batchText}
                  onChange={(event) => setBatchText(event.currentTarget.value)}
                  placeholder="https://example.com/video-1&#10;https://example.com/video-2"
                  spellCheck={false}
                />
                <div className="batch-footer">
                  <span className="batch-meta">
                    {batchUrls.length === 1
                      ? t("batchOneLink")
                      : t("batchManyLinks", { count: batchUrls.length })}
                  </span>
                  <button className="primary-button batch-submit" type="submit" disabled={!canDownloadBatch}>
                    {isDownloading ? t("working") : t("batchDownloadQueue")}
                  </button>
                </div>
              </form>

              {downloadQueue.length > 0 && (
                <section className="queue-panel" aria-label={t("queue")}>
                  <div className="queue-header">
                    <span>{t("queue")}</span>
                    <span>{queueCompletedCount}/{downloadQueue.length}</span>
                  </div>
                  <ProgressBar value={queueProgress} />
                  <div className="queue-list">
                    {downloadQueue.map((item) => (
                      <div
                        className={`queue-item queue-item-${item.status}`}
                        key={item.id}
                      >
                        <span>{queueStatusLabel(item.status, t)}</span>
                        <div className="queue-item-body">
                          <strong>{item.title || item.url}</strong>
                          <div className="queue-item-progress">
                            <ProgressBar value={queueItemProgress(item)} />
                            <small>{formatQueueMeta(item, t)}</small>
                          </div>
                        </div>
                        {item.error && <em>{item.error}</em>}
                      </div>
                    ))}
                  </div>
                </section>
              )}
            </section>
          </div>
        )}

        <div className="dashboard">
          {settingsOpen && preferences && (
            <SettingsPanel
              preferences={preferences}
              t={t}
              onClose={() => setSettingsOpen(false)}
              onSave={saveSettings}
            />
          )}

          {selectedItem && (
            <TrimPanel
              item={selectedItem}
              localPlayPauseShortcut={preferences?.localPlayPauseShortcut ?? "Space"}
              t={t}
              onClose={() => setSelectedItemId(null)}
              onStatus={setStatusText}
            />
          )}

          <section className="panel" aria-label={t("history")}>
            <div className="section-header">
              <h2>{t("history")}</h2>
              <button
                className="text-button"
                type="button"
                disabled={history.length === 0}
                onClick={() => void clearHistory()}
              >
                {t("clear")}
              </button>
            </div>

            <div className="history-list">
              {history.map((item) => (
                <article
                  className={
                    selectedItemId === item.id
                      ? "history-item history-item-selected"
                      : "history-item"
                  }
                  key={item.id}
                  onClick={() => setSelectedItemId(item.id)}
                >
                  <ThumbnailPreview item={item} />

                  <div className="history-content">
                    <div className="history-main">
                      <div className="history-title">{displayTitle(item)}</div>
                      <time className="history-time">{formatDate(item.createdAt)}</time>
                    </div>
                    <div className="history-detail">{sourceName(item.sourceUrl)}</div>
                    <div className="history-detail">{cleanPath(item.filePath)}</div>
                    <div className="history-actions" aria-label={t("historyActions")}>
                      <div className="history-primary-actions">
                        <button
                          className="action-button"
                          type="button"
                          onClick={(event) => {
                            event.stopPropagation();
                            setSelectedItemId(item.id);
                          }}
                        >
                          {t("trim")}
                        </button>
                        <button
                          className="action-button"
                          type="button"
                          onClick={(event) => {
                            event.stopPropagation();
                            void copyHistoryItem(item);
                          }}
                        >
                          {t("copy")}
                        </button>
                      </div>

                      <details
                        className="history-more"
                        onClick={(event) => event.stopPropagation()}
                      >
                        <summary>{t("more")}</summary>
                        <div className="history-menu">
                          <button
                            className="action-button"
                            type="button"
                            onClick={() => void revealHistoryItem(item)}
                          >
                            {t("reveal")}
                          </button>
                          <button
                            className="action-button"
                            type="button"
                            onClick={() => void openHistorySource(item)}
                          >
                            {t("source")}
                          </button>
                          <button
                            className="action-button danger-button"
                            type="button"
                            onClick={() => void deleteHistoryItem(item)}
                          >
                            {t("delete")}
                          </button>
                        </div>
                      </details>
                    </div>
                  </div>
                </article>
              ))}

              {history.length === 0 && (
                <div className="empty-state">
                  {t("historyEmpty")}
                </div>
              )}
            </div>
          </section>
        </div>
      </section>
    </main>
  );
}

function TrimPanel({
  item,
  localPlayPauseShortcut,
  t,
  onClose,
  onStatus,
}: {
  item: DownloadItem;
  localPlayPauseShortcut: LocalPlayPauseShortcut;
  t: Translator;
  onClose: () => void;
  onStatus: (value: string) => void;
}) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [videoSource, setVideoSource] = useState("");
  const [duration, setDuration] = useState(0);
  const [start, setStart] = useState(0);
  const [end, setEnd] = useState(0);
  const [playhead, setPlayhead] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isExporting, setIsExporting] = useState(false);
  const [timelineFrames, setTimelineFrames] = useState<string[]>([]);
  const [framesLoading, setFramesLoading] = useState(false);
  const [errorText, setErrorText] = useState("");

  useEffect(() => {
    let canceled = false;
    setVideoSource("");
    setDuration(0);
    setStart(0);
    setEnd(0);
    setPlayhead(0);
    setIsPlaying(false);
    setTimelineFrames([]);
    setFramesLoading(false);
    setErrorText("");

    void invoke("allow_media_preview", { filePath: item.filePath })
      .then(() => {
        if (!canceled) {
          setVideoSource(convertFileSrc(item.filePath));
        }
      })
      .catch((error) => {
        if (!canceled) {
          setErrorText(formatError(error, t));
        }
      });

    return () => {
      canceled = true;
    };
  }, [item.filePath]);

  useEffect(() => {
    let canceled = false;

    if (!item.filePath || duration <= 0) {
      setTimelineFrames([]);
      setFramesLoading(false);
      return;
    }

    setTimelineFrames([]);
    setFramesLoading(true);

    void invoke<TimelineFramesResult>("generate_timeline_frames", {
      request: {
        sourcePath: item.filePath,
        duration,
        frameCount: 12,
      },
    })
      .then((result) => {
        if (!canceled) {
          setTimelineFrames(result.frames);
        }
      })
      .catch((error) => {
        if (!canceled) {
          setErrorText(formatError(error, t));
        }
      })
      .finally(() => {
        if (!canceled) {
          setFramesLoading(false);
        }
      });

    return () => {
      canceled = true;
    };
  }, [duration, item.filePath]);

  const selection = useMemo<TrimSelection>(() => {
    return {
      start: clampNumber(start, 0, Math.max(duration, 0)),
      end: clampNumber(end, 0, Math.max(duration, 0)),
    };
  }, [duration, end, start]);

  const normalizedSelection = useMemo<TrimSelection>(() => {
    if (selection.end - selection.start >= 0.25) {
      return selection;
    }

    return {
      start: Math.min(selection.start, Math.max(duration - 0.25, 0)),
      end: Math.min(Math.max(selection.start + 0.25, selection.end), duration),
    };
  }, [duration, selection]);

  const canExport =
    !isExporting &&
    duration > 0 &&
    normalizedSelection.end - normalizedSelection.start >= 0.25 &&
    Boolean(videoSource);

  useEffect(() => {
    if (duration <= 0) {
      return;
    }

    const boundedStart = clampNumber(start, 0, Math.max(duration - 0.25, 0));
    const boundedEnd = clampNumber(end, boundedStart + 0.25, duration);
    if (boundedStart !== start) {
      setStart(roundSeconds(boundedStart));
    }
    if (boundedEnd !== end) {
      setEnd(roundSeconds(boundedEnd));
    }
    setPlayhead((value) => clampNumber(value, boundedStart, boundedEnd));
  }, [duration, end, start]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video || duration <= 0) {
      return;
    }

    function handleTimeUpdate() {
      if (!video) {
        return;
      }

      const current = video.currentTime;
      if (!Number.isFinite(current)) {
        return;
      }

      if (current >= normalizedSelection.end - 0.02) {
        video.pause();
        video.currentTime = normalizedSelection.end;
        setIsPlaying(false);
        setPlayhead(normalizedSelection.end);
      } else {
        setPlayhead(clampNumber(current, normalizedSelection.start, normalizedSelection.end));
      }
    }

    function handlePlay() {
      setIsPlaying(true);
    }

    function handlePause() {
      setIsPlaying(false);
    }

    video.addEventListener("timeupdate", handleTimeUpdate);
    video.addEventListener("play", handlePlay);
    video.addEventListener("pause", handlePause);

    return () => {
      video.removeEventListener("timeupdate", handleTimeUpdate);
      video.removeEventListener("play", handlePlay);
      video.removeEventListener("pause", handlePause);
    };
  }, [duration, normalizedSelection.end, normalizedSelection.start]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video || duration <= 0) {
      return;
    }

    if (video.currentTime < normalizedSelection.start || video.currentTime > normalizedSelection.end) {
      video.currentTime = normalizedSelection.start;
      setPlayhead(normalizedSelection.start);
    }
  }, [duration, normalizedSelection.end, normalizedSelection.start]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) {
        return;
      }

      if (event.code !== codeForLocalShortcut(localPlayPauseShortcut)) {
        return;
      }

      event.preventDefault();
      void togglePlayback();
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [localPlayPauseShortcut]);

  function handleLoadedMetadata(event: React.SyntheticEvent<HTMLVideoElement>) {
    const nextDuration = event.currentTarget.duration;
    if (!Number.isFinite(nextDuration) || nextDuration <= 0) {
      setErrorText(t("trimCouldNotReadDuration"));
      return;
    }

    setDuration(nextDuration);
    setStart(0);
    setEnd(roundSeconds(nextDuration));
    setPlayhead(0);
  }

  function seekPreview(seconds: number) {
    const boundedSeconds = clampNumber(seconds, normalizedSelection.start, normalizedSelection.end);
    setPlayhead(boundedSeconds);

    const video = videoRef.current;
    if (video) {
      video.currentTime = boundedSeconds;
    }
  }

  function updateSelection(nextSelection: TrimSelection, previewTime: number) {
    const nextStart = roundSeconds(clampNumber(nextSelection.start, 0, Math.max(duration - 0.25, 0)));
    const nextEnd = roundSeconds(clampNumber(nextSelection.end, nextStart + 0.25, duration));
    setStart(nextStart);
    setEnd(nextEnd);

    const boundedPreviewTime = clampNumber(previewTime, nextStart, nextEnd);
    setPlayhead(boundedPreviewTime);

    const video = videoRef.current;
    if (video) {
      video.currentTime = boundedPreviewTime;
    }
  }

  async function togglePlayback() {
    const video = videoRef.current;
    if (!video || duration <= 0) {
      return;
    }

    if (!video.paused) {
      video.pause();
      setIsPlaying(false);
      return;
    }

    if (
      video.currentTime < normalizedSelection.start ||
      video.currentTime >= normalizedSelection.end - 0.02
    ) {
      video.currentTime = normalizedSelection.start;
      setPlayhead(normalizedSelection.start);
    }

    try {
      await video.play();
      setIsPlaying(true);
    } catch (error) {
      setErrorText(formatError(error, t));
    }
  }

  async function saveTrim() {
    await runTrimCommand("export_trim", t("trimSaved"));
  }

  async function copyTrim() {
    await runTrimCommand("copy_trim", t("trimCopied"));
  }

  async function runTrimCommand(command: "export_trim" | "copy_trim", success: string) {
    setIsExporting(true);
    setErrorText("");
    onStatus(t("trimExporting"));

    try {
      const result = await invoke<TrimResult>(command, {
        request: {
          sourcePath: item.filePath,
          selection: normalizedSelection,
        },
      });
      onStatus(`${success} ${cleanPath(result.outputPath)}`);
    } catch (error) {
      const message = formatError(error, t);
      setErrorText(message);
      onStatus(message);
    } finally {
      setIsExporting(false);
    }
  }

  return (
    <section className="panel trim-panel" aria-label={t("trimSelectedVideo")}>
      <div className="section-header">
        <div>
          <h2>{t("trim")}</h2>
          <p>{displayTitle(item)}</p>
        </div>
        <button className="text-button" type="button" onClick={onClose}>
          {t("close")}
        </button>
      </div>

      <div className="trim-layout">
        <div className="video-frame">
          {videoSource ? (
            <video
              ref={videoRef}
              className="video-preview"
              src={videoSource}
              preload="metadata"
              onLoadedMetadata={handleLoadedMetadata}
              onError={() => setErrorText(t("trimCouldNotLoadPreview"))}
            />
          ) : (
            <div className="video-placeholder">{t("trimLoadingPreview")}</div>
          )}
        </div>

        <TrimTimeline
          duration={duration}
          frames={timelineFrames}
          framesLoading={framesLoading}
          isPlaying={isPlaying}
          playhead={playhead}
          selection={normalizedSelection}
          onPlayPause={() => void togglePlayback()}
          onSeek={seekPreview}
          onSelectionChange={updateSelection}
          t={t}
        />

        <div className="trim-controls">
          <label className="trim-field">
            <span>{t("trimStart")}</span>
            <input
              type="number"
              min={0}
              max={duration || undefined}
              step={0.1}
              value={start}
              onChange={(event) => setStart(inputSeconds(event.currentTarget.value))}
            />
          </label>
          <label className="trim-field">
            <span>{t("trimEnd")}</span>
            <input
              type="number"
              min={0}
              max={duration || undefined}
              step={0.1}
              value={end}
              onChange={(event) => setEnd(inputSeconds(event.currentTarget.value))}
            />
          </label>
          <div className="trim-meta">
            {duration > 0
              ? t("trimSelectedOf", {
                  selected: formatSeconds(normalizedSelection.end - normalizedSelection.start),
                  duration: formatSeconds(duration),
                })
              : t("trimDurationLoading")}
          </div>
          {errorText && <div className="trim-error">{errorText}</div>}
          <div className="trim-actions">
            <button
              className="secondary-button"
              type="button"
              disabled={!canExport}
              onClick={() => void saveTrim()}
            >
              {t("trimSaveClip")}
            </button>
            <button
              className="outline-button"
              type="button"
              disabled={!canExport}
              onClick={() => void copyTrim()}
            >
              {t("trimCopyClip")}
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}

function SettingsPanel({
  preferences,
  t,
  onClose,
  onSave,
}: {
  preferences: Preferences;
  t: Translator;
  onClose: () => void;
  onSave: (preferences: Preferences) => Promise<Preferences>;
}) {
  const [draft, setDraft] = useState(preferences);
  const [saving, setSaving] = useState(false);
  const [errorText, setErrorText] = useState("");

  useEffect(() => {
    setDraft(preferences);
    setErrorText("");
  }, [preferences]);

  async function submitSettings(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaving(true);
    setErrorText("");

    try {
      const saved = await onSave({
        ...draft,
        language: normalizeLanguagePreference(draft.language),
        globalActivationShortcut: draft.globalActivationShortcut.trim(),
      });
      setDraft(saved);
    } catch (error) {
      setErrorText(formatError(error, t));
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="panel settings-panel" aria-label={t("settings")}>
      <form onSubmit={submitSettings}>
        <div className="section-header">
          <div>
            <h2>{t("settings")}</h2>
            <p>{t("settingsSubtitle")}</p>
          </div>
          <button className="text-button" type="button" onClick={onClose}>
            {t("close")}
          </button>
        </div>

        <div className="settings-grid">
          <div className="settings-group">
            <h3>{t("settingsLanguageGroup")}</h3>
            <label className="settings-field">
              <span>{t("settingsLanguage")}</span>
              <select
                value={draft.language}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    language: normalizeLanguagePreference(event.currentTarget.value),
                  })
                }
              >
                {languageOptions.map((option) => (
                  <option value={option.value} key={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="settings-group">
            <h3>{t("settingsShortcuts")}</h3>
            <label className="settings-field">
              <span>{t("settingsLocalPlayPause")}</span>
              <select
                value={draft.localPlayPauseShortcut}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    localPlayPauseShortcut: event.currentTarget
                      .value as LocalPlayPauseShortcut,
                  })
                }
              >
                <option value="Space">Space</option>
                <option value="Enter">Enter</option>
                <option value="P">P</option>
              </select>
            </label>

            <label className="settings-field">
              <span>{t("settingsGlobalActivation")}</span>
              <input
                value={draft.globalActivationShortcut}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    globalActivationShortcut: event.currentTarget.value,
                  })
                }
                placeholder="CommandOrControl+Shift+D"
                spellCheck={false}
              />
            </label>
            <label className="settings-check">
              <input
                type="checkbox"
                checked={draft.globalActivationShortcutEnabled}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    globalActivationShortcutEnabled: event.currentTarget.checked,
                  })
                }
              />
              <span>{t("settingsEnableGlobalShortcut")}</span>
            </label>
          </div>
        </div>

        {errorText && <div className="trim-error">{errorText}</div>}

        <div className="settings-actions">
          <button className="secondary-button" type="submit" disabled={saving}>
            {saving ? t("settingsSaving") : t("settingsSave")}
          </button>
        </div>
      </form>
    </section>
  );
}

type TimelineDragTarget = "start" | "end" | "playhead";

function TrimTimeline({
  duration,
  frames,
  framesLoading,
  isPlaying,
  playhead,
  selection,
  onPlayPause,
  onSeek,
  onSelectionChange,
  t,
}: {
  duration: number;
  frames: string[];
  framesLoading: boolean;
  isPlaying: boolean;
  playhead: number;
  selection: TrimSelection;
  onPlayPause: () => void;
  onSeek: (seconds: number) => void;
  onSelectionChange: (selection: TrimSelection, previewTime: number) => void;
  t: Translator;
}) {
  const trackRef = useRef<HTMLDivElement | null>(null);
  const [dragTarget, setDragTarget] = useState<TimelineDragTarget | null>(null);
  const hasFrames = frames.length > 0;

  useEffect(() => {
    if (!dragTarget) {
      return;
    }

    const activeTarget = dragTarget;

    function handlePointerMove(event: PointerEvent) {
      event.preventDefault();
      updateFromPointer(event.clientX, activeTarget);
    }

    function handlePointerUp() {
      setDragTarget(null);
    }

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp, { once: true });
    window.addEventListener("pointercancel", handlePointerUp, { once: true });

    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerUp);
    };
  }, [dragTarget, duration, selection.end, selection.start]);

  const startPercent = percentForSeconds(selection.start, duration);
  const endPercent = percentForSeconds(selection.end, duration);
  const playheadPercent = percentForSeconds(
    clampNumber(playhead, selection.start, selection.end),
    duration,
  );

  function secondsFromClientX(clientX: number) {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0 || duration <= 0) {
      return 0;
    }

    const fraction = clampNumber((clientX - rect.left) / rect.width, 0, 1);
    return fraction * duration;
  }

  function updateFromPointer(clientX: number, target: TimelineDragTarget) {
    if (duration <= 0) {
      return;
    }

    const seconds = secondsFromClientX(clientX);

    if (target === "start") {
      const nextStart = clampNumber(seconds, 0, selection.end - 0.25);
      onSelectionChange({ start: nextStart, end: selection.end }, nextStart);
      return;
    }

    if (target === "end") {
      const nextEnd = clampNumber(seconds, selection.start + 0.25, duration);
      onSelectionChange({ start: selection.start, end: nextEnd }, nextEnd);
      return;
    }

    onSeek(clampNumber(seconds, selection.start, selection.end));
  }

  function startDrag(target: TimelineDragTarget, event: React.PointerEvent<HTMLButtonElement>) {
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragTarget(target);
    updateFromPointer(event.clientX, target);
  }

  return (
    <div className="timeline-block">
      <div className="timeline-toolbar">
        <button
          className="timeline-play-button"
          type="button"
          onClick={onPlayPause}
          disabled={duration <= 0}
          aria-label={isPlaying ? t("timelinePausePreview") : t("timelinePlayPreview")}
        >
          {isPlaying ? t("timelinePause") : t("timelinePlay")}
        </button>
        <div className="timeline-readout">
          {formatSeconds(playhead)} / {formatSeconds(duration)}
        </div>
      </div>

      <div
        ref={trackRef}
        className="timeline-track"
        role="slider"
        aria-label={t("timelineAria")}
        aria-valuemin={0}
        aria-valuemax={roundSeconds(duration)}
        aria-valuenow={roundSeconds(playhead)}
        onPointerDown={(event) => {
          event.preventDefault();
          onSeek(secondsFromClientX(event.clientX));
        }}
      >
        <div className={hasFrames ? "timeline-frames" : "timeline-frames timeline-empty"}>
          {hasFrames
            ? frames.map((frame, index) => (
                <img
                  className="timeline-frame"
                  src={frame}
                  alt=""
                  draggable={false}
                  key={`${frame.length}-${index}`}
                />
              ))
            : Array.from({ length: 12 }, (_, index) => (
                <span className="timeline-frame-placeholder" key={index} />
              ))}
        </div>

        <div className="timeline-dim timeline-dim-left" style={{ width: `${startPercent}%` }} />
        <div
          className="timeline-dim timeline-dim-right"
          style={{ left: `${endPercent}%`, width: `${100 - endPercent}%` }}
        />
        <div
          className="timeline-selection"
          style={{
            left: `${startPercent}%`,
            width: `${Math.max(endPercent - startPercent, 0)}%`,
          }}
        />
        <div className="timeline-playhead" style={{ left: `${playheadPercent}%` }} />

        <button
          className="timeline-handle timeline-handle-start"
          type="button"
          aria-label={t("timelineStart")}
          style={{ left: `${startPercent}%` }}
          onPointerDown={(event) => startDrag("start", event)}
        />
        <button
          className="timeline-handle timeline-handle-end"
          type="button"
          aria-label={t("timelineEnd")}
          style={{ left: `${endPercent}%` }}
          onPointerDown={(event) => startDrag("end", event)}
        />
        <button
          className="timeline-playhead-hit"
          type="button"
          aria-label={t("timelinePreviewPosition")}
          style={{ left: `${playheadPercent}%` }}
          onPointerDown={(event) => startDrag("playhead", event)}
        />
      </div>

      <div className="timeline-caption">
        {framesLoading ? t("timelineGeneratingFrames") : `${formatSeconds(selection.start)} - ${formatSeconds(selection.end)}`}
      </div>
    </div>
  );
}

function ThumbnailPreview({ item }: { item: DownloadItem }) {
  const [source, setSource] = useState("");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let canceled = false;
    setSource("");
    setFailed(false);

    if (!item.thumbnailPath) {
      return;
    }

    void invoke<string>("thumbnail_data_url", {
      thumbnailPath: item.thumbnailPath,
    })
      .then((value) => {
        if (!canceled) {
          setSource(value);
        }
      })
      .catch(() => {
        if (!canceled) {
          setFailed(true);
        }
      });

    return () => {
      canceled = true;
    };
  }, [item.thumbnailPath]);

  if (!item.thumbnailPath || failed || !source) {
    return <div className="thumbnail-placeholder" aria-hidden="true" />;
  }

  return (
    <img
      className="thumbnail-image"
      src={source}
      alt=""
      loading="lazy"
      onError={() => setFailed(true)}
    />
  );
}

function ProgressBar({ value }: { value: number | null }) {
  const progress = normalizeProgress(value);

  if (progress === null) {
    return (
      <div className="progress-bar progress-bar-indeterminate" role="progressbar">
        <span />
      </div>
    );
  }

  return (
    <div
      className="progress-bar"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(progress)}
    >
      <span style={{ width: `${progress}%` }} />
    </div>
  );
}

function looksLikeWebUrl(value: string) {
  try {
    const parsed = new URL(value.trim());
    return ["http:", "https:"].includes(parsed.protocol) && parsed.host.length > 0;
  } catch {
    return false;
  }
}

function parseUrls(value: string) {
  const seen = new Set<string>();
  const matches = value.match(/https?:\/\/[^\s<>"']+/gi) ?? [];
  const urls: string[] = [];

  for (const match of matches) {
    const sourceUrl = match.replace(/[),.;\]]+$/g, "");
    if (!looksLikeWebUrl(sourceUrl) || seen.has(sourceUrl)) {
      continue;
    }

    seen.add(sourceUrl);
    urls.push(sourceUrl);
  }

  if (urls.length === 0 && looksLikeWebUrl(value.trim())) {
    return [value.trim()];
  }

  return urls;
}

type Translator = ReturnType<typeof createTranslator>;

function queueStatusLabel(status: QueueItem["status"], t: Translator) {
  switch (status) {
    case "running":
      return t("queueRunning");
    case "success":
      return t("queueDone");
    case "error":
      return t("queueFailed");
    case "pending":
    default:
      return t("queuePending");
  }
}

function queueItemProgress(item: QueueItem) {
  if (item.status === "success" || item.status === "error") {
    return 100;
  }

  if (item.status === "pending") {
    return 0;
  }

  return item.progress;
}

function formatQueueMeta(item: QueueItem, t: Translator) {
  if (item.status === "pending") {
    return t("queueWaiting");
  }

  if (item.status === "error") {
    return t("queueFailed");
  }

  if (item.status === "success") {
    return "100%";
  }

  return formatTransferMeta(item, t);
}

function formatTransferMeta(value: { progress: number | null; speed?: string; eta?: string }, t: Translator) {
  const parts = [formatProgress(value.progress, t)];
  if (value.speed) {
    parts.push(value.speed);
  }
  if (value.eta) {
    parts.push(t("progressEta", { eta: value.eta }));
  }

  return parts.join(" · ");
}

function formatProgress(value: number | null, t: Translator) {
  const progress = normalizeProgress(value);
  return progress === null ? t("progressStarting") : `${Math.round(progress)}%`;
}

function normalizeProgress(value: number | null | undefined) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return null;
  }

  return Math.min(Math.max(value, 0), 100);
}

function matchesDownloadProgress(id: string, url: string, event: DownloadProgressEvent) {
  if (event.requestId) {
    return event.requestId === id;
  }

  return event.sourceUrl === url;
}

function createDownloadRequestId() {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function formatError(error: unknown, t?: Translator) {
  if (typeof error === "string") {
    return error;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return t ? t("errorFallback") : "Could not complete the request.";
}

function formatDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "";
  }

  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function folderName(value?: string) {
  if (!value) {
    return "";
  }

  const parts = cleanPath(value).split(/[\\/]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : value;
}

function displayTitle(item: DownloadItem) {
  if (item.title && !looksMojibake(item.title)) {
    return item.title;
  }

  return fileName(item.filePath) || item.sourceUrl;
}

function cleanPath(value?: string) {
  if (!value) {
    return "";
  }

  return value.replace(/^\\\\\?\\/, "");
}

function fileName(value: string) {
  const parts = cleanPath(value).split(/[\\/]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : value;
}

function inputSeconds(value: string) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function clampNumber(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) {
    return min;
  }

  return Math.min(Math.max(value, min), max);
}

function roundSeconds(value: number) {
  if (!Number.isFinite(value)) {
    return 0;
  }

  return Math.round(value * 10) / 10;
}

function formatSeconds(value: number) {
  if (!Number.isFinite(value) || value < 0) {
    return "0.0s";
  }

  return `${value.toFixed(1)}s`;
}

function codeForLocalShortcut(value: LocalPlayPauseShortcut) {
  switch (value) {
    case "Enter":
      return "Enter";
    case "P":
      return "KeyP";
    case "Space":
    default:
      return "Space";
  }
}

function percentForSeconds(value: number, duration: number) {
  if (!Number.isFinite(value) || !Number.isFinite(duration) || duration <= 0) {
    return 0;
  }

  return clampNumber((value / duration) * 100, 0, 100);
}

function looksMojibake(value: string) {
  return value.includes("\u951f\u65a4\u62f7") || value.includes("\uFFFD");
}

function sourceName(value: string) {
  try {
    const host = new URL(value).host.replace(/^www\./, "");
    return host || value;
  } catch {
    return value;
  }
}

function requirePreferences(preferences: Preferences | null): Preferences {
  return (
    preferences ?? {
      downloadFolder: "",
      language: "system",
      localPlayPauseShortcut: "Space",
      globalActivationShortcutEnabled: false,
      globalActivationShortcut: "CommandOrControl+Shift+D",
      updateCheckEnabled: false,
    }
  );
}

export default App;

