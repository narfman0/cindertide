(archetype open-steppe
  (name "Open Steppe")
  (description "Flat open terrain. No cover. Vehicles dominate. Pure maneuver warfare.")
  (params
    (forest-patches 2 :min 0 :max 5))
  (steps
    (fill Grass)
    (forests forest-patches :placement random)
    (resources 8 :placement scattered)
    (spawn-zones 4 :layout corners :clear-radius 6)))
