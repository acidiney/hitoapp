import type { Schema } from "../../../shared/schema";
import { useLayoutEffect, useRef, useState } from "react";
import { Link } from "@inertiajs/react";
import { ChevronLeft, ChevronRight, Settings } from "lucide-react";

import { useDiscovery } from "../hooks/useDiscovery";
import { usePlayer } from "../../playback/components/PlaybackProvider";
import DiscoverySearch from "./DiscoverySearch";
import ProviderResults from "./ProviderResults";
import EmptyState from "../../../shared/ui/EmptyState";
import SectionWarning from "../../../shared/ui/SectionWarning";
import Button from "../../../shared/ui/Button";

export default function ProviderSearch({
  user,
  connections,
  service_names,
}: Pick<
  Schema<"DiscoverPageProps">,
  "user" | "connections" | "service_names"
>) {
  const providers = connections.filter((connection) =>
    connection.manifest.capabilities.includes("search"),
  );
  const [choice, setChoice] = useState<{ account: string; id: string } | null>(
    null,
  );
  const selected =
    providers.find(
      (provider) => choice?.account === user.id && provider.id === choice.id,
    ) ??
    providers[0] ??
    null;
  const discovery = useDiscovery(user.id, selected, usePlayer());
  const heading = useRef<HTMLHeadingElement>(null);
  const focusResults = useRef(false);
  useLayoutEffect(() => {
    if (focusResults.current && !discovery.pending) {
      focusResults.current = false;
      heading.current?.focus();
    }
  }, [discovery.pending, discovery.items]);
  function turn(direction: "next" | "previous") {
    focusResults.current = true;
    void discovery[direction]();
  }
  return (
    <section className="discover-section" aria-labelledby="discover-services">
      <div className="discover-section-head">
        <h2 id="discover-services">Search your services</h2>
        {selected && (
          <Link className="button secondary" href="/settings?section=plugins">
            <Settings size={16} />
            Manage services
          </Link>
        )}
      </div>
      {!selected ? (
        <div className="discovery-empty">
          <p>
            {connections.length
              ? "These services don’t offer music search. Connect another service to search its catalogue here."
              : "Connect a music service to search its catalogue here."}
          </p>
          <Link className="button primary" href="/settings?section=plugins">
            Connect a music service
          </Link>
        </div>
      ) : (
        <>
          <DiscoverySearch
            key={user.id}
            connections={providers}
            names={service_names}
            selected={selected.id}
            pending={discovery.pending}
            onChoose={(id) => setChoice({ account: user.id, id })}
            onSearch={discovery.search}
            pills
          />
          {!selected.manifest.capabilities.includes("sources") && (
            <p className="discovery-note">
              This service finds tracks but provides no audio. Added tracks play
              through your other services.
            </p>
          )}
          {discovery.error && (
            <SectionWarning
              tone="error"
              title={`${service_names[selected.service_id] ?? selected.manifest.name} could not finish.`}
            >
              {discovery.error}{" "}
              <Link
                className="text-link"
                href="/discover"
                only={["connections", "service_names"]}
                preserveState
                preserveScroll
                replace
              >
                Refresh services
              </Link>
            </SectionWarning>
          )}
          {discovery.query && (
            <div className="discovery-results-heading">
              <h2 ref={heading} tabIndex={-1}>
                Search results
              </h2>
              <span className="muted" role="status">
                {discovery.pending
                  ? "Searching…"
                  : discovery.error && discovery.items.length === 0
                    ? "Search could not finish"
                    : `${discovery.items.length} ${discovery.items.length === 1 ? "track" : "tracks"} · Page ${discovery.page + 1}`}
              </span>
            </div>
          )}
          {discovery.items.length > 0 ? (
            <ProviderResults
              items={discovery.items}
              saved={discovery.saved}
              selection={discovery.selection}
              pending={discovery.pending}
              canPlay={selected.manifest.capabilities.includes("sources")}
              onSelect={(hit, intent) => void discovery.select(hit, intent)}
            />
          ) : discovery.pending ? (
            <div
              className="discovery-loading"
              aria-label="Loading search results"
              aria-hidden="true"
            >
              {Array.from({ length: 5 }, (_, index) => (
                <div key={index}>
                  <span />
                  <span />
                </div>
              ))}
            </div>
          ) : discovery.error ? null : (
            <EmptyState
              title={
                discovery.query
                  ? "No matching tracks"
                  : `Search ${service_names[selected.service_id] ?? selected.manifest.name}`
              }
              text={
                discovery.query
                  ? "Try another track or artist, or choose a different service."
                  : "Find a track, then play it or add it to your library."
              }
              libraryLink={false}
            />
          )}
          {(discovery.hasNext || discovery.hasPrevious) && (
            <nav
              className="discovery-pagination"
              aria-label="Search result pages"
            >
              <Button
                disabled={!discovery.hasPrevious || discovery.pending}
                onClick={() => turn("previous")}
              >
                <ChevronLeft size={16} />
                Previous
              </Button>
              <span>Page {discovery.page + 1}</span>
              <Button
                disabled={!discovery.hasNext || discovery.pending}
                onClick={() => turn("next")}
              >
                Next
                <ChevronRight size={16} />
              </Button>
            </nav>
          )}
        </>
      )}
    </section>
  );
}
