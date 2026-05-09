(archetype choke-point
  (name "Choke Point")
  (description "Two open bases connected by narrow forest corridors. Control the bottlenecks.")
  (params
    (choke-count 3 :min 1 :max 5)
    (choke-width 2 :min 1 :max 4)
    (player-count 2 :min 2 :max 4))
  (steps
    (fill Grass)
    (forests choke-count :placement flanks)
    (rubble 2 :placement center)
    (resources 6 :placement scattered)
    (spawn-zones player-count :layout corners :clear-radius 7)))
