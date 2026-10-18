UPDATE titles
SET min_availability = 'released'
WHERE facet = 'movie'
  AND min_availability IS NULL;
