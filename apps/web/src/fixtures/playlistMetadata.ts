import { createElement } from "react";
import { router } from "@inertiajs/react";
import PlaylistPage from "../pages/PlaylistPage";
import { NoticeProvider } from "../components/Notices";
import { PlaybackProvider } from "../player/PlaybackProvider";
import type {
  PlaylistPageProps,
  PluginInstallation,
  PluginSettings,
} from "../types";
import { copiedPlaylistPage } from "./playlistPages";
import { playlistConnection } from "./playlistImports";

export const metadataProps = copiedPlaylistPage;
const retainedOrigin = metadataProps.detail.origin;
if (!retainedOrigin)
  throw new Error("The metadata fixture needs an imported origin");
export const metadataOrigin = retainedOrigin;

export const metadataInstallation: PluginInstallation = {
  ...playlistConnection,
  id: metadataOrigin.installation_id,
  service_id: metadataOrigin.service_id,
  revision: metadataOrigin.installation_revision + 4,
  manifest: { ...playlistConnection.manifest, id: metadataOrigin.provider },
};

export const metadataSettings: PluginSettings = {
  credential_storage_ready: true,
  installations: [metadataInstallation],
  services: [
    {
      id: metadataOrigin.service_id,
      name: "Playlist metadata fixture",
      base_url: "https://metadata-provider.invalid/",
      revision: metadataOrigin.service_revision + 1,
    },
  ],
};

export const metadataPath = `/api/v1/playlists/${metadataProps.detail.playlist.id}/metadata/refresh`;
export const metadataDownloadPath = `/api/v1/playlists/${metadataProps.detail.playlist.id}/download`;

export function metadataPage(props: PlaylistPageProps = metadataProps) {
  return createElement(
    NoticeProvider,
    null,
    createElement(PlaybackProvider, {
      account: props.user.id,
      children: createElement(PlaylistPage, props),
    }),
  );
}

export function heldMetadataResponse() {
  let resolve!: (response: Response) => void;
  const response = new Promise<Response>((done) => {
    resolve = done;
  });
  return { response, resolve };
}

type ReloadOptions = NonNullable<Parameters<typeof router.reload>[0]>;
type ReloadVisit = Parameters<NonNullable<ReloadOptions["onFinish"]>>[0];
type ReloadPage = Parameters<NonNullable<ReloadOptions["onSuccess"]>>[0];

export function metadataReloadVisit(
  options: ReloadOptions,
  completed = false,
): ReloadVisit {
  const noop = () => {};
  return {
    id: "owned-playlist-metadata-detail-reload",
    url: new URL(
      `http://localhost/playlists/${metadataProps.detail.playlist.id}`,
    ),
    method: "get",
    data: {},
    completed,
    cancelled: false,
    interrupted: false,
    replace: false,
    preserveScroll: true,
    preserveState: true,
    only: options.only ?? [],
    except: [],
    headers: {},
    errorBag: null,
    forceFormData: false,
    queryStringArrayFormat: "brackets",
    async: true,
    showProgress: false,
    prefetch: false,
    fresh: true,
    reset: [],
    preserveUrl: true,
    preserveErrors: false,
    invalidateCacheTags: [],
    viewTransition: false,
    component: null,
    pageProps: null,
    cached: false,
    onCancelToken: options.onCancelToken ?? noop,
    onBefore: options.onBefore ?? noop,
    onBeforeUpdate: options.onBeforeUpdate ?? noop,
    onStart: options.onStart ?? noop,
    onProgress: options.onProgress ?? noop,
    onFinish: options.onFinish ?? noop,
    onCancel: options.onCancel ?? noop,
    onSuccess: options.onSuccess ?? noop,
    onError: options.onError ?? noop,
    onHttpException: options.onHttpException ?? noop,
    onNetworkError: options.onNetworkError ?? noop,
    onFlash: options.onFlash ?? noop,
    onPrefetched: options.onPrefetched ?? noop,
    onPrefetching: options.onPrefetching ?? noop,
  };
}

export function metadataReloadPage(
  props: PlaylistPageProps = metadataProps,
): ReloadPage {
  return {
    component: "PlaylistPage",
    url: `/playlists/${props.detail.playlist.id}`,
    version: null,
    props: { ...props, errors: {} },
    rescuedProps: [],
    flash: {},
    rememberedState: {},
  };
}
