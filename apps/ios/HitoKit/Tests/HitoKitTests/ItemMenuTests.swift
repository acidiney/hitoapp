import Testing
@testable import HitoKit

@Suite struct ItemMenuTests {
    func track(_ id: String = "r1", available: Bool = true) -> Track {
        Track(id: id, title: "Song", artist: "Band", album: nil, releaseId: nil, durationMs: 1000, available: available, playsFromServices: false, liked: false)
    }

    func titles(_ actions: [ItemAction]) -> [String] { actions.map(\.title) }

    @Test func aTrackOffersItsActionsInOrder() {
        let actions = ItemMenu.track(track(), liked: false, artworkMissing: false)
        #expect(titles(actions) == ["Play next", "Track details", "Fix source", "Like"])
    }

    @Test func aLikedTrackOffersRemovingTheLikeInTheSamePlace() {
        let unliked = ItemMenu.track(track(), liked: false, artworkMissing: false)
        let liked = ItemMenu.track(track(), liked: true, artworkMissing: false)
        #expect(unliked.firstIndex(of: .like) == liked.firstIndex(of: .unlike))
        #expect(ItemAction.unlike.title == "Remove from Liked tracks")
        #expect(!liked.contains(.like))
    }

    @Test func aTrackWithoutAPlayerOffersNoLike() {
        let actions = ItemMenu.track(track(), liked: nil, artworkMissing: false)
        #expect(!actions.contains(.like) && !actions.contains(.unlike))
    }

    @Test func theSourceActionFollowsWhetherTheTrackPlays() {
        #expect(ItemMenu.track(track(available: false), liked: nil, artworkMissing: false).contains(.findAudio))
        #expect(ItemMenu.track(track(available: true), liked: nil, artworkMissing: false).contains(.fixSource))
        let unlinked = ItemMenu.track(track("entry:9", available: false), liked: nil, artworkMissing: false)
        #expect(!unlinked.contains(.findAudio) && !unlinked.contains(.fixSource))
    }

    @Test func missingArtworkCanBeRetriedFromEveryKind() {
        let playlist = Playlist(id: "p1", name: "Mix", kind: .local)
        #expect(ItemMenu.track(track(), liked: nil, artworkMissing: true).last == .retryArtwork)
        #expect(ItemMenu.album(artworkMissing: true).last == .retryArtwork)
        #expect(ItemMenu.playlist(playlist, ready: 1, artworkMissing: true).contains(.retryArtwork))
        #expect(!ItemMenu.album(artworkMissing: false).contains(.retryArtwork))
    }

    @Test func anAlbumOffersPlayingAndChoosingTheArtist() {
        #expect(titles(ItemMenu.album(artworkMissing: false)) == ["Play", "Play next", "Choose the artist"])
    }

    @Test func aLocalPlaylistCanBeEditedAndDeleted() {
        let playlist = Playlist(id: "p1", name: "Mix", kind: .local)
        #expect(titles(ItemMenu.playlist(playlist, ready: 3, artworkMissing: false)) == ["Play", "Play next", "Edit playlist", "Delete playlist"])
    }

    @Test func aFollowedPlaylistCanBeDeletedButNotEdited() {
        let playlist = Playlist(id: "p1", name: "Mix", kind: .followed)
        #expect(titles(ItemMenu.playlist(playlist, ready: 3, artworkMissing: false)) == ["Play", "Play next", "Delete playlist"])
    }

    @Test func aPlaylistWithNothingReadyOffersNoPlaying() {
        let playlist = Playlist(id: "p1", name: "Mix", kind: .local)
        let actions = ItemMenu.playlist(playlist, ready: 0, artworkMissing: false)
        #expect(!actions.contains(.play) && !actions.contains(.playNext))
    }

    @Test func aPlaylistNotOpenedYetOffersPlaying() {
        let playlist = Playlist(id: "p1", name: "Mix", kind: .local)
        #expect(ItemMenu.playlist(playlist, ready: nil, artworkMissing: false).starts(with: [.play, .playNext]))
    }

    @Test func artistsAndListsPlayOrShuffle() {
        #expect(titles(ItemMenu.collection) == ["Play", "Shuffle"])
    }

    @Test func discoverOffersRadiosAndFeaturedPlaylists() {
        #expect(titles(ItemMenu.radio) == ["Start radio"])
        #expect(titles(ItemMenu.featured) == ["Play", "Open"])
    }

    @Test func aQueuedTrackMovesOrLeaves() {
        #expect(titles(ItemMenu.queued) == ["Move up", "Move down", "Remove from queue"])
    }

    @Test func onlyRemovingAndDeletingAreDestructive() {
        let destructive = ItemAction.allCases.filter(\.isDestructive)
        #expect(Set(destructive) == [.removeFromQueue, .removeFromPlaylist, .delete])
    }
}
