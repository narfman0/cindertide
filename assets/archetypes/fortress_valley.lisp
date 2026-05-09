(archetype fortress-valley
  (name "Fortress Valley")
  (description "One base in dense forest cover, one on open plains. Asymmetric attacker/defender.")
  (params
    (forest-depth 12 :min 6 :max 20))
  (steps
    (fill Grass)
    (forests 1 :placement flanks)
    (ridgelines 1)
    (rubble 2 :placement center)
    (resources 6 :placement scattered)
    (spawn-zones 2 :layout corners :clear-radius 5)))
