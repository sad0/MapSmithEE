--[[
SadoQoL rest camp: the camp's lasting sight (John 2026-11-02). No fog of war inside SQCampSite.VISION, an ellipse
round the fire ({ x, y, radius x, radius y } in area pixels, written by E:/AI/bg3d/tools/build_camp3d.py), as if a
Farsight were cast there for ever, with no effect shown. Creatures inside are seen like in a party member's sight.

How: the area's visibility map (CGameArea + 0xbb0, CVisibilityMap) holds one u16 per 32 x 32 px tile; bit 15 is
"explored", bits 0-14 "visible now". Party viewers only ever write their own bit (1 << slot, slots 0-7), so bit 14 is
never set or cleared by the engine: set here, it keeps the tile visible. The map is rebuilt when the area loads, so the
bits are put back every camp tick (SQCamp_Hooks, every 500 ms). Offsets: IEX's analysis of BG2EE 2.7.3.

INTERIM file from the Camp Map 3D branch, for testing in EET 2.7.3 (dev); the SadoEE lead moves it into SadoQoL.
--]]

SQCamp_Hooks = SQCamp_Hooks or {}

local VISMAP, SIGHT = 0xbb0, 0xC000   -- explored + the camp's own visible bit (14)

SQCamp_Hooks.vision = function()
	local v = SQCampSite and SQCampSite.VISION
	if not v then return end
	local area = EEex_Area_GetVisible()
	if not area or string.upper(area.m_resref:get()) ~= 'SQCAMP' then return end
	local map = EEex_UDToPtr(area) + VISMAP
	local tiles = EEex_ReadPtr(map)
	if tiles == 0 then return end
	local w, h = EEex_ReadU16(map + 0xc), EEex_ReadU16(map + 0xe)
	local cx, cy, rx, ry = v[1], v[2], v[3], v[4]
	for ty = math.max(0, math.floor((cy - ry) / 32)), math.min(h - 1, math.floor((cy + ry) / 32)) do
		for tx = math.max(0, math.floor((cx - rx) / 32)), math.min(w - 1, math.floor((cx + rx) / 32)) do
			local dx, dy = (tx * 32 + 16 - cx) / rx, (ty * 32 + 16 - cy) / ry
			if dx * dx + dy * dy <= 1 then
				local p = tiles + 2 * (ty * w + tx)
				local t = EEex_ReadU16(p)
				if EEex_BAnd(t, SIGHT) ~= SIGHT then EEex_WriteU16(p, EEex_BOr(t, SIGHT)) end
			end
		end
	end
	-- creatures are not drawn from the tiles: each one carries a "seen by the party" countdown (CGameObject + 0x4c,
	-- i16) that the engine refills from a party member's line of sight and counts down every AI tick. Topped up here
	-- for everyone inside the circle (30 ticks, about 1 s: longer than this hook's 500 ms), the camp's people stay seen.
	EEex_Utility_IterateCPtrList(area.m_lVertSort, function(id)
		local o = EEex_GameObject_Get(id)
		if not (o and o.m_objectType == 49) then return end
		local dx, dy = (o.m_pos.x - cx) / rx, (o.m_pos.y - cy) / ry
		if dx * dx + dy * dy <= 1 then EEex_WriteU16(EEex_UDToPtr(o) + 0x4c, 30) end
	end)
end
