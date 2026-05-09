(archetype urban-ruin
  (name "Urban Ruin")
  (description "A bombed-out city. Dense rubble limits sight lines. Infantry excels here.")
  (params
    (ruin-density 0.35 :min 0.1 :max 0.6))
  (steps
    (fill Grass)
    (urban-ruins ruin-density)
    (resources 4 :placement scattered)
    (spawn-zones 4 :layout corners :clear-radius 8)))
