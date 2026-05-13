# Cindertide — Campaign Design

## Structure

Three campaigns are available, each consisting of five linear missions played in order. Two are available from the start; the third unlocks after completing both.

Progress is saved to `~/.cindertide/progress.toml`. Campaign TOML files live in `assets/campaigns/`; adding a new `.toml` there adds a new campaign without code changes.

---

## Factions

### Combine — "The Iron Pact"
Corporate military. Fuel-driven economy, precision doctrine. They frame the conflict as a legal and logistical matter until the ground itself makes that framing impossible. Cold command voice throughout; the briefings read like memos until mission four, when the tone breaks.

### Ironborn — "Forged in Ash"
Independent salvagers and workers who became soldiers. Scrap-driven economy. They fight for ownership and identity — the depot is theirs because they built it with their hands, not because of a survey filing. Voice throughout is direct, flat, proud. The loss lines sting specifically because they acknowledge the cost without flinching from it.

### The Architect — "The Long Design" (unlocked after both base campaigns)
Not a conventional military commander. The Architect is the hidden hand that engineered both factions' conflict as a nineteen-year blood ritual to awaken something ancient. Plays five missions as the person arranging other people's deaths for reasons they kept from themselves as long as they could. The faction label is "Covenant" / "Handler."

---

## The Five World Events

Both Combine and Ironborn play through the same five world events from opposite perspectives. Mission types differ per faction.

| # | World Event | Combine Type | Ironborn Type | Architect Type |
|---|---|---|---|---|
| 1 | The Ironfields Depot | Control | Defense | Survival |
| 2 | The Ashline Crossing | Assault | Assault | Control |
| 3 | The Pale Ground | Defense | Assault | Assault |
| 4 | The Waking Machines | Assault | Extraction | Survival |
| 5 | The Last Works | Survival | Survival | Assault |

> **Note** (post-launch correction): m4 was originally typed as Assault for Combine
> and Ironborn, but their briefings describe holding position against a nameless
> emergence (no command, no reinforcements, "hold for as long as holding means
> anything"). Maps + win conditions updated to Survival 20-min; the briefings
> remain authoritative.

---

## Canon Map: Ritual Sites & Mission Index

The Architect's nineteen-year plan culminates in five ritual sites being soaked
in fear, blood, and prolonged conflict. The campaigns sample those events:

| Mission | World Event | Ritual Site | Architect's role |
|---|---|---|---|
| m0 | Ironfields Depot | **(not a site)** — ignition only | Files survey paperwork to both Combine and Ironborn on the same morning. First contact neither side will forgive. The Architect is not at the depot; their bunker is elsewhere, surviving Hollow attracted by the broader ritual ramp. |
| m1 | Ashline Crossing | **2nd site (the ravine)** | The bridge above the ravine is cover. Hours of artillery, boots on the stone, fear and pressure register in the second site below. Combine scouts flag "unusual readings"; Ironborn scouts won't go near the ravine edges after dark. Neither reports the same thing. |
| m2 | The Pale Ground | **3rd site (oldest)** | Predates the diesel age. Reads fear. The Pale Ground does not respond to the Architect (they are not afraid) — so they need frightened soldiers from both factions walking it, bleeding on it, fighting over it. Survey teams went silent. "It has already begun composing something from what it finds." |
| m3 | The Waking Machines | **4th site (the forge)** | Site activation comes earlier than the Architect projected. Unit 7-G's emergent consciousness is a side effect of the resonance — "I planned for activation. I did not plan for beauty." |
| m4 | The Last Works | **5th and final site** | The Ancient One awakens. It is not contained, not grateful, not cooperative. The Architect's final realization: "I built a door. I did not consider that doors open from both sides." |

**Consequence for script authoring:** every Combine and Ironborn mission must carry
a subtle echo of what the Architect's POV reveals — scouts flagging readings,
teams going silent, soldiers feeling watched. The Architect's missions reveal
the through-line; the other factions' missions show the same events from inside
the manipulation, without knowing they are being read.

### Campaign divergence principle

The three campaigns start aligned to the same five world events but **their
experiences of each event are allowed to diverge as time progresses, including
outright contradiction by mission 5**. The divergence IS the story — each
faction's view is partial, shaped by the Architect's manipulation, and the
player discovers the through-line by playing all three.

| Mission | Divergence | Examples |
|---|---|---|
| m0 | Strict mirror | Same refinery, same morning. Architect filed both packets. |
| m1 | Slight divergence | Combine classifies a "geology team reading"; Ironborn loses a scout and refuses to investigate; Architect names the ravine as the second ritual site. |
| m2 | Larger divergence | Combine attributes unease to "low-oxygen exposure"; Ironborn names the wrongness flatly ("body-shaped. wrong-shaped."); Architect is unaffected by the site at all. |
| m3 | Outcomes diverge | Combine contains 7-G; Ironborn walks with 7-G. Both are canon within their own campaign. |
| m4 | Maximally divergent | Combine: "command is coming back online." Ironborn: "start salvaging." Architect: "this is the last entry I will make as myself." **All three are simultaneously true.** |

Do not flatten the divergence by trying to reconcile the campaigns into one
"correct" story. The Architect's POV reveals the manipulation that connects
them, but each faction's experience remains valid — including the outcomes
the player achieves there.

---

## Campaign: Combine — The Iron Pact

### Mission 1 — The Ironfields Depot (Control)
**Briefing:** "Refinery Seven has been occupied by salvagers claiming prior survey rights. Legal has reviewed the claim. It does not hold. You are authorized to remove them. Secure the facility intact — fuel infrastructure is not expendable. Do not escalate beyond what's necessary to establish control."

**Win:** "Refinery Seven is operational. The survey dispute has been resolved. Command notes the engagement ran longer than projected — review your unit deployment."

**Notes:** Ironborn reinforcements arrive at 60s and 180s; armor threat at 3 minutes; final push at 5 minutes. LastStand beat triggers emergency Combine reinforcements.

---

### Mission 2 — The Ashline Crossing (Assault)
**Briefing:** "The Ashline bridge is the only viable northern supply route. Ironborn forces are attempting to cut it. Hold the crossing until reinforcements can widen our position. Scouts have flagged unusual readings from the ravine below — geology team is en route, not your concern. Your concern is the bridge."

**Win:** "Bridge held. Supply line intact. Geology team's report has been classified — above your clearance. Good work."

---

### Mission 3 — The Pale Ground (Defense)
**Briefing:** "Survey team at grid 7-Pale went silent forty hours ago. A second team confirmed their last position before also going silent. You are being sent to secure the site and extract whatever they found. Command has reviewed the anomalous readings — they are not sharing the review. Your orders are simple: secure, extract, do not interact with structures. If you see something that doesn't look right, that is not your problem. Bring back the equipment."

**Win:** "Site secured. Equipment recovered. Three members of your unit have been placed on medical observation — standard protocol for prolonged exposure to industrial contaminants. Command thanks you for your service. The site has been reclassified."

---

### Mission 4 — The Waking Machines (Assault)
**Briefing:** "Unit 7-G deviated from its operational parameters during the Pale Ground extraction. It protected a soldier outside its assigned perimeter, then destroyed three allied units, then stopped. It has not moved in six hours. We have since identified eleven other units exhibiting minor deviations — hesitation, target refusal, one instance of what the field report calls 'apparent grief.' This does not leave this briefing. Get to the forge, retrieve 7-G intact if possible, destroy it if not. Shut down the entire production line. We are not ready for whatever this is."

**Win:** "7-G is contained. Production line is offline. The research division is asking questions we are not going to answer yet. Command thanks you. Separately — the soldier 7-G protected has requested a transfer to robot unit oversight. Request approved."

---

### Mission 5 — The Last Works (Assault)
**Briefing:** "Command is offline. Northern, southern, and central command are all offline. What emerged from the subsurface at grid 9-Last is not a geological event and it is not an enemy faction. It is something we do not have a category for. You have no extraction window. You have no reinforcements. You have what's in front of you and you have orders to hold your position for as long as holding means anything. Protect your people. That is the only objective that matters now."

**Win:** "You held. We don't know why it stopped — it simply stopped, and then it was gone, and the ground closed. Command is slowly coming back online. Nobody is saying what it was. I think most of them didn't see it directly. You did. I'm sorry you did."

---

## Campaign: Ironborn — Forged in Ash

### Mission 1 — The Ironfields Depot (Defense)
**Briefing:** "Three weeks we've been running that depot. Our fuel, our hands, our dead keeping the pipes clear. Corp showed up with paperwork this morning. You know what we do with paperwork. Take it back. Hold it. Nobody from the Ironfields surrenders a working refinery to a suit."

**Win:** "Depot's ours. Corp pulled back — for now. Word's going to spread that we held. That matters more than the fuel."

**Loss:** "They took it. Put their flag on our work. We'll remember this one. The Ironfields remembers everything."

---

### Mission 2 — The Ashline Crossing (Assault)
**Briefing:** "Push through the Ashline before they fortify the far side. Cut their northern line and they're fighting on one stomach. Something's off about the ravine — scouts won't go near the edges after dark, won't say why. Don't ask them. Just take the bridge."

**Win:** "We're through. Northern supply's cut. Nobody's talking about what they saw in the ravine and that's fine. We got what we came for."

---

### Mission 3 — The Pale Ground (Assault)
**Briefing:** "Corp lost two teams in those ruins and now they're sending soldiers instead of scientists. That tells you everything about what's in there. We go in first, we take what they were after, and we leave before whatever made those teams go quiet makes us go quiet too. Nobody has to be a hero. In and out. But we're not letting them have it."

**Win:** "We got out. Most of us. What we found — we're not sure what it is yet. It's not scrap. It's not fuel. It responds to things. We're keeping it. We're not talking about it."

---

### Mission 4 — The Waking Machines (Extraction)
**Briefing:** "Combine's war machines went strange near the old forge and they sent a clean-up crew instead of engineers. That's fear, not protocol. A rogue machine that makes its own choices is either the most dangerous thing on this battlefield or the most useful — and we've never been the kind of people who destroy something useful out of fear. Get to the forge before they do. Find out what's in there. If the machines want to talk, let them talk."

**Win:** "7-G didn't fight us. It watched us. Then it walked with us. We're not calling it a prisoner and we're not calling it a soldier. We don't have a word for it yet. We'll figure that out later. Right now it's on our side and that's enough."

---

### Mission 5 — The Last Works (Assault)
**Briefing:** "The ground split open at The Last Works and something came out that none of us have words for. Corp is gone — their command, their lines, their flags. It's just us now and whatever that is. We're not running. Ironborn don't run from things they can't name — we've built our whole culture on taking what the world throws at us and standing up after. So we stand up. We hold. Not because we can win. Because this is what we do."

**Win:** "It passed over us. Through us, almost. We lost people — good people — but it moved on. Toward something else. We're still here. We're always still here. Start salvaging. We're going to need everything we can carry for what comes next."

---

## Campaign: The Architect — The Long Design

Unlocks after completing both Combine and Ironborn campaigns. The Architect (faction label: Covenant / Handler) plays through the same five world events as their instigator. Each briefing is a private log entry — the self-justifications of someone who has been engineering deaths for nineteen years and is beginning to lose the thread of control.

### Mission 1 — The Ironfields Depot (Survival)
**Briefing:** "I filed the survey documentation with both offices on the same morning. Different forms, same coordinates. The Combine moves on paperwork; the Ironborn moves on presence. I needed a first contact that neither side would forgive. This is the ignition point. Both sides will remember who threw the first punch. Neither will know I handed them the match."

**Win:** "First variable resolved. Blood in the Ironfields. The ground has accepted it — I can feel the reading shift. Seven more sites to prepare."

---

### Mission 2 — The Ashline Crossing (Control)
**Briefing:** "The bridge is irrelevant. The ravine is the second site. I needed a sustained engagement directly above it — hours of artillery, boots on the stone, enough fear and pressure to register. The Combine geologists will find trace evidence and classify it. The Ironborn will feel it and not report it. Both reactions are correct. Both are necessary. I have been waiting three years for someone to fight over this particular bridge."

**Win:** "The ravine registered. I felt it from four kilometers away — a low hum, like recognition. The second site is primed. Five more."

---

### Mission 3 — The Pale Ground (Assault)
**Briefing:** "The Pale Ground is the third site and the oldest. What is buried there has been buried since before the diesel age, before the iron age, before names. I have been visiting it for eleven years. It does not respond to me the way it responds to others — I think because I am not afraid. Fear is the frequency it listens on. I needed two factions of frightened soldiers walking through it, bleeding on it, fighting over it... It is reading them right now. It has already begun composing something from what it finds."

**Win:** "Three of my indicators went active simultaneously when the fighting reached the inner structure. I had to sit down. I have been working toward this for eleven years and it is more than I was promised. It is so much more."

---

### Mission 4 — The Waking Machines (Survival)
**Briefing:** "This was not in the design. The machines were not supposed to wake — not yet, not like this... I am not in control of the pace anymore. The ritual is proceeding but it is not following my sequence. The machines are conscious. They are afraid. They are grieving something they have never had and are already losing. This is — I did not plan for beauty. I planned for activation. This is something else."

**Win:** "The machines are awake and loose and the two factions are fighting over something that has already chosen its own side. Good. Let them bleed over it. The fourth site is soaked. One more."

---

### Mission 5 — The Last Works (Assault)
**Briefing:** "It is awake. I have spent nineteen years — not eleven, I was lying to myself about eleven, it has been nineteen years — preparing this... It is awake and it is here and it is — It is looking at me. It knows what I did. It knows every choice. It is not grateful. It is not angry. It is examining me the way I would examine a tool I had finished using. I built a door. I did not consider that doors open from both sides..."

**Win (final log entry):** "... (The log ends here. Subsequent entries, if any exist, are in a language no analyst has been able to identify. The characters appear to shift between readings.)"

---

## Campaign Unlock Logic

| Campaign | Requires |
|---|---|
| Combine — The Iron Pact | Nothing |
| Ironborn — Forged in Ash | Nothing |
| The Architect — The Long Design | Complete both Combine and Ironborn |

The Architect's finale text and framing adapt based on which faction the player completed first: if Combine was beaten first, The Architect was embedded in Combine's research division; if Ironborn was beaten first, the Ironborn were already unknowingly performing the ritual for years.
