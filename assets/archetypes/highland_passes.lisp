(archetype highland-passes
  (name "Highland Passes")
  (description "Forest ridgelines on the flanks with an exposed central valley.")
  (params
    (ridgeline-count 2 :min 1 :max 4))
  (steps
    (fill Grass)
    (ridgelines ridgeline-count)
    (rubble 2 :placement center)
    (resources 6 :placement flanks)
    (spawn-zones 2 :layout mirror :clear-radius 6)))
