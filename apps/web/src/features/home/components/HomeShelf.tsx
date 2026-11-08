import type { ReactNode } from "react";
import { Link } from "@inertiajs/react";
import Artwork from "../../artwork/components/Artwork";
import ShelfRow from "../../../shared/ui/ShelfRow";
import MediaCard from "../../../shared/ui/MediaCard";
import type { ShelfCard } from "../model/homeShelves";

function ShelfCover({ card }: { card: ShelfCard }) {
  const { cover } = card;
  if (cover.kind === "artist")
    return (
      <Artwork
        recording={cover.recording}
        fallback={{ id: cover.id, title: card.title, kind: "artist" }}
      />
    );
  return (
    <Artwork
      release={cover.id}
      fallback={{ id: cover.id, title: card.title, artist: cover.artist }}
    />
  );
}

export default function HomeShelf({
  id,
  title,
  cards,
  more,
  empty,
}: {
  id: string;
  title: string;
  cards: ShelfCard[];
  more?: { href: string; label: string };
  empty?: ReactNode;
}) {
  return (
    <section className="home-shelf" aria-labelledby={id}>
      <div className="home-shelf-head">
        <h2 id={id}>{title}</h2>
        {more && (
          <Link className="text-link" href={more.href}>
            {more.label}
          </Link>
        )}
      </div>
      {cards.length === 0 && empty ? (
        <p className="home-hint">{empty}</p>
      ) : (
        <ShelfRow label={title}>
          {cards.map((card) => (
            <li key={card.key}>
              <MediaCard
                title={card.title}
                subtitle={card.subtitle || undefined}
                href={card.href}
                round={card.cover.kind === "artist"}
                cover={<ShelfCover card={card} />}
              />
            </li>
          ))}
        </ShelfRow>
      )}
    </section>
  );
}
