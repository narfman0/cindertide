(archetype river-crossing
  (name "River Crossing")
  (description "A winding river bisects the map. Bridges are the only way through.")
  (params
    (bridge-count 2 :min 1 :max 4)
    (meander 0.4 :min 0.0 :max 1.0)
    (river-width 2 :min 1 :max 4))
  (steps
    (fill Grass)
    (river :at center-y :meander meander :width river-width)
    (bridges bridge-count :terrain Road)
    (forests 3 :placement flanks)
    (rubble 2 :placement center)
    (resources 6 :placement scattered)
    (spawn-zones 2 :layout mirror :clear-radius 6)))
