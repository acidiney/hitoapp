import { Link } from "@inertiajs/react";
import {
  credentialsRequiredMessage,
  pluginSettingsHref,
} from "../model/credentialError";

export default function PluginSettingsLink() {
  return <Link href={pluginSettingsHref}>Open plugin settings</Link>;
}

export function PluginErrorText({ message }: { message: string }) {
  return (
    <>
      {message}
      {message === credentialsRequiredMessage && (
        <>
          {" "}
          <PluginSettingsLink />
        </>
      )}
    </>
  );
}
