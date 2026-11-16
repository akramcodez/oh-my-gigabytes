"use client";

import { useEffect, useState } from "react";
import { LuGithub, LuStar } from "react-icons/lu";
import { starCount } from "@/lib/github";
import { formatCount } from "@/lib/npm";
import { REPO } from "@/lib/site";

export default function Stars({ initial }: { initial: number | null }) {
  const [stars, setStars] = useState(initial);

  useEffect(() => {
    let live = true;
    starCount().then((n) => {
      if (live && n !== null) setStars(n);
    });
    return () => {
      live = false;
    };
  }, []);

  return (
    <a href={REPO} className="star-link" aria-label={stars === null ? "GitHub" : `GitHub, ${stars} stars`}>
      <LuGithub aria-hidden="true" />
      <span className="nav-label">GitHub</span>
      {stars !== null && (
        <span className="star-count" aria-hidden="true">
          <LuStar />
          {formatCount(stars)}
        </span>
      )}
    </a>
  );
}
