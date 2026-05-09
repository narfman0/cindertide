(archetype island-chain
  (name "Island Chain")
  (description "Multiple landmasses linked by bridges. Multi-front warfare. Great for multiplayer.")
  (params
    (island-count 4 :min 3 :max 6))
  (steps
    (fill Mud)
    (islands island-count)
    (resources 8 :placement scattered)
    (bases :player bottom-left :enemy top-right :clear-radius 5)))
