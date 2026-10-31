import { Link } from "@inertiajs/react";
import Artwork from "../../artwork/components/Artwork";
import type { ShelfCard } from "../model/homeShelves";

function ShelfCover({ card }: { card: ShelfCard }) {
  const { cover } = card;
  if (cover.kind === "artist")
    return (
      <Artwork
        recording={cover.recording}
        fallback={{ id: cover.id, title: card.title }}
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
}: {
  id: string;
  title: string;
  cards: ShelfCard[];
  more?: { href: string; label: string };
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
      <ul className="home-shelf-row">
        {cards.map((card) => (
          <li key={card.key}>
            <Link
              className="home-card"
              data-round={card.cover.kind === "artist" || undefined}
              href={card.href}
            >
              <span className="home-card-cover">
                <ShelfCover card={card} />
              </span>
              <span className="home-card-title">{card.title}</span>
              {card.subtitle && (
                <span className="home-card-subtitle">{card.subtitle}</span>
              )}
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
