(archetype industrial-complex
  (name "Industrial Complex")
  (description "Resource-rich factory ruins at the center. Control the economy or lose.")
  (params
    (ruin-clusters 4 :min 2 :max 8)
    (resource-count 10 :min 4 :max 16))
  (steps
    (fill Grass)
    (rubble ruin-clusters :placement center)
    (forests 2 :placement flanks)
    (resources resource-count :placement center)
    (spawn-zones 4 :layout sides :clear-radius 6)))
